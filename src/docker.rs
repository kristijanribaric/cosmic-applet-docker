use bollard::container::{
    ListContainersOptions, LogsOptions, RestartContainerOptions, StartContainerOptions,
    StopContainerOptions, Stats, StatsOptions,
};
use bollard::Docker;
use cosmic::iced::Subscription;
use cosmic::iced_futures::stream;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum ContainerState {
    Running,
    Stopped,
    Restarting,
    Paused,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: ContainerState,
    pub status: String,
}

#[derive(Debug, Clone, Default)]
pub struct ContainerStats {
    pub cpu_percent: f64,
    pub memory_usage_mb: f64,
    pub memory_limit_mb: f64,
    pub memory_percent: f64,
}

#[derive(Debug, Clone)]
pub enum DockerEvent {
    ContainersUpdated(Result<Vec<ContainerInfo>, String>),
    StatsUpdated(HashMap<String, ContainerStats>),
}

fn parse_state(state: &str) -> ContainerState {
    match state {
        "running" => ContainerState::Running,
        "exited" | "dead" => ContainerState::Stopped,
        "restarting" => ContainerState::Restarting,
        "paused" => ContainerState::Paused,
        other => ContainerState::Other(other.to_string()),
    }
}

fn calculate_cpu_percent(stats: &Stats) -> f64 {
    let cpu_stats = &stats.cpu_stats;
    let precpu_stats = &stats.precpu_stats;

    let cpu_delta = cpu_stats.cpu_usage.total_usage as f64
        - precpu_stats.cpu_usage.total_usage as f64;
    let system_delta = cpu_stats.system_cpu_usage.unwrap_or(0) as f64
        - precpu_stats.system_cpu_usage.unwrap_or(0) as f64;

    if system_delta > 0.0 && cpu_delta >= 0.0 {
        let num_cpus = cpu_stats.online_cpus.unwrap_or(1) as f64;
        (cpu_delta / system_delta) * num_cpus * 100.0
    } else {
        0.0
    }
}

fn calculate_memory(stats: &Stats) -> (f64, f64, f64) {
    let usage = stats.memory_stats.usage.unwrap_or(0) as f64;
    let limit = stats.memory_stats.limit.unwrap_or(1) as f64;
    let cache = stats
        .memory_stats
        .stats
        .as_ref()
        .and_then(|s| match s {
            bollard::container::MemoryStatsStats::V1(v1) => Some(v1.cache),
            bollard::container::MemoryStatsStats::V2(v2) => Some(v2.inactive_file),
        })
        .unwrap_or(0) as f64;

    let actual_usage = usage - cache;
    let usage_mb = actual_usage / 1_048_576.0;
    let limit_mb = limit / 1_048_576.0;
    let percent = if limit > 0.0 {
        (actual_usage / limit) * 100.0
    } else {
        0.0
    };
    (usage_mb, limit_mb, percent)
}

pub fn container_list_subscription(popup_open: bool) -> Subscription<DockerEvent> {
    let interval = if popup_open {
        Duration::from_secs(3)
    } else {
        Duration::from_secs(10)
    };

    let id = if popup_open {
        "docker-list-fast"
    } else {
        "docker-list-slow"
    };

    Subscription::run_with_id(
        id,
        stream::channel(10, move |mut output| async move {
            loop {
                let result = fetch_containers().await;
                let _ = output.send(DockerEvent::ContainersUpdated(result)).await;
                tokio::time::sleep(interval).await;
            }
        }),
    )
}

pub fn container_stats_subscription(container_ids: Vec<String>) -> Subscription<DockerEvent> {
    if container_ids.is_empty() {
        return Subscription::none();
    }

    Subscription::run_with_id(
        "docker-stats",
        stream::channel(10, move |mut output| async move {
            loop {
                let stats = fetch_stats(&container_ids).await;
                let _ = output.send(DockerEvent::StatsUpdated(stats)).await;
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }),
    )
}

async fn fetch_containers() -> Result<Vec<ContainerInfo>, String> {
    let docker = Docker::connect_with_local_defaults().map_err(|e| e.to_string())?;

    let options = ListContainersOptions::<String> {
        all: true,
        ..Default::default()
    };

    let containers = docker
        .list_containers(Some(options))
        .await
        .map_err(|e| e.to_string())?;

    Ok(containers
        .into_iter()
        .map(|c| {
            let id = c.id.unwrap_or_default();
            let name = c
                .names
                .and_then(|n| n.first().cloned())
                .unwrap_or_default()
                .trim_start_matches('/')
                .to_string();
            let image = c.image.unwrap_or_default();
            let state_str = c.state.unwrap_or_default();
            let status = c.status.unwrap_or_default();

            ContainerInfo {
                id,
                name,
                image,
                state: parse_state(&state_str),
                status,
            }
        })
        .collect())
}

async fn fetch_stats(container_ids: &[String]) -> HashMap<String, ContainerStats> {
    let docker = match Docker::connect_with_local_defaults() {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };

    let mut results = HashMap::new();

    for id in container_ids {
        let options = StatsOptions {
            stream: false,
            one_shot: true,
        };

        let mut stats_stream = docker.stats(id, Some(options));
        if let Some(Ok(stats)) = stats_stream.next().await {
            let cpu = calculate_cpu_percent(&stats);
            let (mem_usage, mem_limit, mem_percent) = calculate_memory(&stats);
            results.insert(
                id.clone(),
                ContainerStats {
                    cpu_percent: cpu,
                    memory_usage_mb: mem_usage,
                    memory_limit_mb: mem_limit,
                    memory_percent: mem_percent,
                },
            );
        }
    }

    results
}

pub async fn start_container(id: String) -> Result<String, String> {
    let docker = Docker::connect_with_local_defaults().map_err(|e| e.to_string())?;
    docker
        .start_container(&id, None::<StartContainerOptions<String>>)
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

pub async fn stop_container(id: String) -> Result<String, String> {
    let docker = Docker::connect_with_local_defaults().map_err(|e| e.to_string())?;
    docker
        .stop_container(&id, Some(StopContainerOptions { t: 10 }))
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

pub async fn restart_container(id: String) -> Result<String, String> {
    let docker = Docker::connect_with_local_defaults().map_err(|e| e.to_string())?;
    docker
        .restart_container(&id, Some(RestartContainerOptions { t: 10 }))
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

pub async fn fetch_logs(id: String) -> Result<(String, String), String> {
    let docker = Docker::connect_with_local_defaults().map_err(|e| e.to_string())?;

    let options = LogsOptions::<String> {
        stdout: true,
        stderr: true,
        tail: "100".to_string(),
        ..Default::default()
    };

    let logs: Vec<_> = docker.logs(&id, Some(options)).collect().await;

    let mut output = String::new();
    for log in logs {
        match log {
            Ok(line) => {
                output.push_str(&line.to_string());
            }
            Err(e) => return Err(e.to_string()),
        }
    }

    Ok((id, output))
}
