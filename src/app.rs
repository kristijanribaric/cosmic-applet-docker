use crate::config::APP_ID;
use crate::docker::{
    self, ContainerDetails, ContainerInfo, ContainerState, ContainerStats, DockerEvent,
    HealthStatus, PortMapping,
};
use crate::fl;
use cosmic::app::Core;
use cosmic::iced::platform_specific::shell::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Length, Limits, Subscription};
use cosmic::iced_runtime::core::window;
use cosmic::widget::{self, scrollable, text};
use cosmic::{Action, Element, Task};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    DockerEvent(DockerEvent),
    StartContainer(String),
    StopContainer(String),
    RestartContainer(String),
    ActionCompleted(Result<String, String>),
    ShowLogs(String, String),
    BackToList,
    OpenInBrowser(u16),
    SearchChanged(String),
    ClearSearch,
    ToggleGroup(String),
    StopAll,
    StartAll,
    StopGroup(String),
    StartGroup(String),
    DeleteContainer(String),
    ConfirmDelete(String),
    CancelDelete,
    CopyContainerId(String),
    ShowDetails(String, String),
    DetailsReceived(Result<(String, ContainerDetails), String>),
}

#[derive(Debug, Clone, PartialEq)]
enum PopupView {
    ContainerList,
    ContainerLogs,
    ContainerDetails,
}

pub struct DockerApplet {
    core: Core,
    popup: Option<Id>,
    docker_available: bool,
    containers: Vec<ContainerInfo>,
    stats: HashMap<String, ContainerStats>,
    current_view: PopupView,
    log_container_name: String,
    log_container_id: String,
    log_content: String,
    logs_loading: bool,
    pending_ops: HashSet<String>,
    health: HashMap<String, HealthStatus>,
    details_container_name: String,
    details_data: Option<ContainerDetails>,
    details_loading: bool,
    search_query: String,
    collapsed_groups: HashSet<String>,
    confirm_delete: Option<String>,
    user_initiated_stops: HashSet<String>,
}

impl cosmic::Application for DockerApplet {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Action<Self::Message>>) {
        let applet = DockerApplet {
            core,
            popup: None,
            docker_available: true,
            containers: Vec::new(),
            stats: HashMap::new(),
            current_view: PopupView::ContainerList,
            log_container_name: String::new(),
            log_container_id: String::new(),
            log_content: String::new(),
            logs_loading: false,
            pending_ops: HashSet::new(),
            health: HashMap::new(),
            details_container_name: String::new(),
            details_data: None,
            details_loading: false,
            search_query: String::new(),
            collapsed_groups: HashSet::new(),
            confirm_delete: None,
            user_initiated_stops: HashSet::new(),
        };
        (applet, Task::none())
    }

    fn update(&mut self, message: Self::Message) -> Task<Action<Self::Message>> {
        match message {
            Message::TogglePopup => {
                return if let Some(popup_id) = self.popup.take() {
                    self.current_view = PopupView::ContainerList;
                    self.log_content.clear();
                    self.log_container_id.clear();
                    self.search_query.clear();
                    self.confirm_delete = None;
                    self.details_data = None;
                    destroy_popup(popup_id)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);
                    self.current_view = PopupView::ContainerList;

                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );

                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(400.0)
                        .min_width(320.0)
                        .min_height(100.0)
                        .max_height(600.0);

                    get_popup(popup_settings)
                };
            }

            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                    self.current_view = PopupView::ContainerList;
                    self.log_content.clear();
                    self.log_container_id.clear();
                    self.search_query.clear();
                    self.confirm_delete = None;
                    self.details_data = None;
                }
            }

            Message::DockerEvent(event) => match event {
                DockerEvent::ContainersUpdated(Ok(containers)) => {
                    self.docker_available = true;
                    self.containers = containers;
                }
                DockerEvent::ContainersUpdated(Err(_)) => {
                    self.docker_available = false;
                    self.containers.clear();
                    self.stats.clear();
                }
                DockerEvent::StatsUpdated(stats) => {
                    self.stats = stats;
                }
                DockerEvent::HealthUpdated(h) => {
                    self.health = h;
                }
                DockerEvent::LogLine(id, line) => {
                    if id == self.log_container_id {
                        self.logs_loading = false;
                        self.log_content.push_str(&line);
                    }
                }
                DockerEvent::ContainerLifecycleEvent {
                    action,
                    container_id,
                    container_name,
                    attributes,
                } => {
                    if action == "die" {
                        if !self.user_initiated_stops.remove(&container_id) {
                            let _ = notify_rust::Notification::new()
                                .summary("Docker")
                                .body(&fl!(
                                    "container-stopped",
                                    name = container_name.as_str()
                                ))
                                .icon("dialog-warning-symbolic")
                                .show();
                        }
                    }
                    if action == "health_status" {
                        let health_status = attributes
                            .get("health_status")
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        if health_status == "unhealthy" {
                            let _ = notify_rust::Notification::new()
                                .summary("Docker")
                                .body(&fl!(
                                    "container-unhealthy",
                                    name = container_name.as_str()
                                ))
                                .icon("dialog-warning-symbolic")
                                .show();
                        }
                    }
                }
            },

            Message::StartContainer(id) => {
                self.pending_ops.insert(id.clone());
                return cosmic::task::future(async move {
                    Message::ActionCompleted(docker::start_container(id).await)
                });
            }

            Message::StopContainer(id) => {
                self.pending_ops.insert(id.clone());
                self.user_initiated_stops.insert(id.clone());
                return cosmic::task::future(async move {
                    Message::ActionCompleted(docker::stop_container(id).await)
                });
            }

            Message::RestartContainer(id) => {
                self.pending_ops.insert(id.clone());
                self.user_initiated_stops.insert(id.clone());
                return cosmic::task::future(async move {
                    Message::ActionCompleted(docker::restart_container(id).await)
                });
            }

            Message::ActionCompleted(result) => match &result {
                Ok(id) => {
                    self.pending_ops.remove(id);
                }
                Err(e) => {
                    tracing::error!("Container action failed: {}", e);
                    self.pending_ops.clear();
                }
            },

            Message::ShowLogs(id, name) => {
                self.current_view = PopupView::ContainerLogs;
                self.log_container_name = name;
                self.log_container_id = id;
                self.log_content.clear();
                self.logs_loading = true;
            }

            Message::BackToList => {
                self.current_view = PopupView::ContainerList;
                self.log_content.clear();
                self.log_container_id.clear();
                self.details_data = None;
            }

            Message::OpenInBrowser(port) => {
                let _ = open::that(format!("http://localhost:{}", port));
            }

            Message::SearchChanged(q) => {
                self.search_query = q;
            }

            Message::ClearSearch => {
                self.search_query.clear();
            }

            Message::ToggleGroup(name) => {
                if !self.collapsed_groups.remove(&name) {
                    self.collapsed_groups.insert(name);
                }
            }

            Message::StopAll => {
                let ids: Vec<String> = self
                    .containers
                    .iter()
                    .filter(|c| c.state == ContainerState::Running)
                    .map(|c| c.id.clone())
                    .collect();
                for id in &ids {
                    self.pending_ops.insert(id.clone());
                    self.user_initiated_stops.insert(id.clone());
                }
                return cosmic::task::future(async move {
                    let mut last_result = Ok(String::new());
                    for id in ids {
                        last_result = docker::stop_container(id).await;
                        if last_result.is_err() {
                            break;
                        }
                    }
                    Message::ActionCompleted(last_result)
                });
            }

            Message::StartAll => {
                let ids: Vec<String> = self
                    .containers
                    .iter()
                    .filter(|c| c.state != ContainerState::Running)
                    .map(|c| c.id.clone())
                    .collect();
                for id in &ids {
                    self.pending_ops.insert(id.clone());
                }
                return cosmic::task::future(async move {
                    let mut last_result = Ok(String::new());
                    for id in ids {
                        last_result = docker::start_container(id).await;
                        if last_result.is_err() {
                            break;
                        }
                    }
                    Message::ActionCompleted(last_result)
                });
            }

            Message::StopGroup(group_name) => {
                let ids: Vec<String> = self
                    .containers
                    .iter()
                    .filter(|c| {
                        c.state == ContainerState::Running
                            && c.labels.get("com.docker.compose.project")
                                == Some(&group_name)
                    })
                    .map(|c| c.id.clone())
                    .collect();
                for id in &ids {
                    self.pending_ops.insert(id.clone());
                    self.user_initiated_stops.insert(id.clone());
                }
                return cosmic::task::future(async move {
                    let mut last_result = Ok(String::new());
                    for id in ids {
                        last_result = docker::stop_container(id).await;
                        if last_result.is_err() {
                            break;
                        }
                    }
                    Message::ActionCompleted(last_result)
                });
            }

            Message::StartGroup(group_name) => {
                let ids: Vec<String> = self
                    .containers
                    .iter()
                    .filter(|c| {
                        c.state != ContainerState::Running
                            && c.labels.get("com.docker.compose.project")
                                == Some(&group_name)
                    })
                    .map(|c| c.id.clone())
                    .collect();
                for id in &ids {
                    self.pending_ops.insert(id.clone());
                }
                return cosmic::task::future(async move {
                    let mut last_result = Ok(String::new());
                    for id in ids {
                        last_result = docker::start_container(id).await;
                        if last_result.is_err() {
                            break;
                        }
                    }
                    Message::ActionCompleted(last_result)
                });
            }

            Message::DeleteContainer(id) => {
                self.confirm_delete = Some(id);
            }

            Message::ConfirmDelete(id) => {
                self.confirm_delete = None;
                self.pending_ops.insert(id.clone());
                return cosmic::task::future(async move {
                    Message::ActionCompleted(docker::remove_container(id).await)
                });
            }

            Message::CancelDelete => {
                self.confirm_delete = None;
            }

            Message::CopyContainerId(id) => {
                let short_id = if id.len() > 12 {
                    id[..12].to_string()
                } else {
                    id.clone()
                };
                let _ = std::process::Command::new("wl-copy")
                    .arg(&short_id)
                    .spawn();
            }

            Message::ShowDetails(id, name) => {
                self.current_view = PopupView::ContainerDetails;
                self.details_container_name = name;
                self.details_data = None;
                self.details_loading = true;
                return cosmic::task::future(async move {
                    Message::DetailsReceived(docker::fetch_container_details(id).await)
                });
            }

            Message::DetailsReceived(result) => {
                self.details_loading = false;
                match result {
                    Ok((_id, details)) => {
                        self.details_data = Some(details);
                    }
                    Err(e) => {
                        tracing::error!("Failed to fetch container details: {}", e);
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let running_count = self
            .containers
            .iter()
            .filter(|c| c.state == ContainerState::Running)
            .count();

        if running_count > 0 {
            let btn = self
                .core
                .applet
                .icon_button("cosmic-applet-docker-symbolic")
                .on_press(Message::TogglePopup);
            widget::row()
                .push(btn)
                .push(text::body(format!("{}", running_count)))
                .align_y(Alignment::Center)
                .spacing(4)
                .into()
        } else {
            self.core
                .applet
                .icon_button("cosmic-applet-docker-symbolic")
                .on_press(Message::TogglePopup)
                .into()
        }
    }

    fn view_window(&self, id: Id) -> Element<'_, Self::Message> {
        if self.popup != Some(id) {
            return text::body("").into();
        }

        let content: Element<Message> = match &self.current_view {
            PopupView::ContainerList => self.view_container_list(),
            PopupView::ContainerLogs => self.view_logs(),
            PopupView::ContainerDetails => self.view_details(),
        };

        self.core
            .applet
            .popup_container(content)
            .max_width(400.0)
            .max_height(600.0)
            .into()
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let popup_open = self.popup.is_some();

        let mut subs = vec![
            docker::container_list_subscription(popup_open).map(Message::DockerEvent),
            docker::docker_events_subscription().map(Message::DockerEvent),
        ];

        if popup_open && self.current_view == PopupView::ContainerList {
            let running_ids: Vec<String> = self
                .containers
                .iter()
                .filter(|c| c.state == ContainerState::Running)
                .map(|c| c.id.clone())
                .collect();

            subs.push(
                docker::container_stats_subscription(running_ids.clone()).map(Message::DockerEvent),
            );
            subs.push(docker::health_subscription(running_ids).map(Message::DockerEvent));
        }

        if popup_open
            && self.current_view == PopupView::ContainerLogs
            && !self.log_container_id.is_empty()
        {
            subs.push(
                docker::log_streaming_subscription(self.log_container_id.clone())
                    .map(Message::DockerEvent),
            );
        }

        Subscription::batch(subs)
    }
}

impl DockerApplet {
    fn view_container_list(&self) -> Element<'_, Message> {
        let mut content = widget::column().spacing(8).width(Length::Fill).padding([0, 12]);

        // Header
        let running_count = self
            .containers
            .iter()
            .filter(|c| c.state == ContainerState::Running)
            .count();

        let header = text::heading(format!(
            "{} · {} running",
            fl!("docker-containers"),
            running_count
        ))
        .width(Length::Fill);

        content = content.push(widget::container(header).padding(8));

        if !self.docker_available {
            content = content.push(
                widget::container(text::body(fl!("docker-unavailable")))
                    .padding(16)
                    .width(Length::Fill)
                    .center_x(Length::Fill),
            );
            return scrollable(content).height(Length::Shrink).into();
        }

        // Search bar
        let search = widget::text_input::search_input(fl!("search-placeholder"), &self.search_query)
            .on_input(Message::SearchChanged)
            .on_clear(Message::ClearSearch);
        content = content.push(search);

        // Bulk action buttons
        let bulk_actions = widget::row()
            .push(
                widget::button::text(fl!("start-all"))
                    .on_press(Message::StartAll)
                    .class(cosmic::theme::Button::Standard),
            )
            .push(
                widget::button::text(fl!("stop-all"))
                    .on_press(Message::StopAll)
                    .class(cosmic::theme::Button::Standard),
            )
            .spacing(8);
        content = content.push(bulk_actions);

        if self.containers.is_empty() {
            content = content.push(
                widget::container(text::body(fl!("no-containers")))
                    .padding(16)
                    .width(Length::Fill)
                    .center_x(Length::Fill),
            );
            return scrollable(content).height(Length::Shrink).into();
        }

        // Filter containers by search query
        let query = self.search_query.to_lowercase();
        let filtered: Vec<&ContainerInfo> = self
            .containers
            .iter()
            .filter(|c| {
                if query.is_empty() {
                    return true;
                }
                c.name.to_lowercase().contains(&query)
                    || c.image.to_lowercase().contains(&query)
            })
            .collect();

        if filtered.is_empty() {
            content = content.push(
                widget::container(text::body(fl!("no-containers")))
                    .padding(16)
                    .width(Length::Fill)
                    .center_x(Length::Fill),
            );
            return scrollable(content).height(Length::Shrink).into();
        }

        // Group by compose project
        let mut compose_groups: BTreeMap<String, Vec<&ContainerInfo>> = BTreeMap::new();
        let mut ungrouped: Vec<&ContainerInfo> = Vec::new();

        for container in &filtered {
            if let Some(project) = container.labels.get("com.docker.compose.project") {
                compose_groups
                    .entry(project.clone())
                    .or_default()
                    .push(container);
            } else {
                ungrouped.push(container);
            }
        }

        let has_groups = !compose_groups.is_empty();

        // Render compose groups
        for (group_name, group_containers) in &compose_groups {
            let running_in_group = group_containers
                .iter()
                .filter(|c| c.state == ContainerState::Running)
                .count();
            let total_in_group = group_containers.len();
            let is_collapsed = self.collapsed_groups.contains(group_name);

            let arrow_icon = if is_collapsed {
                "go-next-symbolic"
            } else {
                "go-down-symbolic"
            };

            let group_header = widget::row()
                .push(
                    widget::button::icon(widget::icon::from_name(arrow_icon))
                        .extra_small()
                        .on_press(Message::ToggleGroup(group_name.clone())),
                )
                .push(
                    text::body(fl!(
                        "compose-group",
                        name = group_name.as_str(),
                        running = running_in_group.to_string(),
                        total = total_in_group.to_string()
                    ))
                    .width(Length::Fill),
                )
                .push(
                    widget::button::icon(widget::icon::from_name(
                        "media-playback-start-symbolic",
                    ))
                    .extra_small()
                    .tooltip(fl!("start-all"))
                    .on_press(Message::StartGroup(group_name.clone())),
                )
                .push(
                    widget::button::icon(widget::icon::from_name(
                        "media-playback-stop-symbolic",
                    ))
                    .extra_small()
                    .tooltip(fl!("stop-all"))
                    .on_press(Message::StopGroup(group_name.clone())),
                )
                .align_y(Alignment::Center)
                .spacing(4)
                .padding([4, 8]);

            content = content.push(group_header);
            content = content.push(widget::divider::horizontal::light());

            if !is_collapsed {
                // Running first, then stopped
                let mut sorted = group_containers.clone();
                sorted.sort_by_key(|c| c.state != ContainerState::Running);

                for container in sorted {
                    if container.state == ContainerState::Running {
                        content = content.push(self.view_running_container(container));
                    } else {
                        content = content.push(self.view_stopped_container(container));
                    }
                    content = content.push(widget::divider::horizontal::light());
                }
            }
        }

        // Render ungrouped containers
        if has_groups && !ungrouped.is_empty() {
            let other_header = widget::row()
                .push(text::caption(fl!("other-containers")))
                .padding([4, 8]);
            content = content.push(other_header);
            content = content.push(widget::divider::horizontal::light());
        }

        // Running containers (ungrouped)
        let running: Vec<&ContainerInfo> = ungrouped
            .iter()
            .filter(|c| c.state == ContainerState::Running)
            .copied()
            .collect();

        for container in &running {
            content = content.push(self.view_running_container(container));
            content = content.push(widget::divider::horizontal::light());
        }

        // Stopped containers (ungrouped)
        let stopped: Vec<&ContainerInfo> = ungrouped
            .iter()
            .filter(|c| c.state != ContainerState::Running)
            .copied()
            .collect();

        if !stopped.is_empty() {
            if !has_groups {
                let stopped_header = widget::row()
                    .push(text::caption(format!(
                        "{} ({})",
                        fl!("stopped"),
                        stopped.len()
                    )))
                    .padding([4, 8]);
                content = content.push(stopped_header);
                content = content.push(widget::divider::horizontal::light());
            }

            for container in &stopped {
                content = content.push(self.view_stopped_container(container));
                content = content.push(widget::divider::horizontal::light());
            }
        }

        scrollable(content).height(Length::Shrink).into()
    }

    fn view_running_container<'a>(&'a self, container: &'a ContainerInfo) -> Element<'a, Message> {
        let is_pending = self.pending_ops.contains(&container.id);

        let stats_text = if let Some(stats) = self.stats.get(&container.id) {
            format!(
                "CPU {:.1}%  ·  MEM {}",
                stats.cpu_percent,
                format_memory(stats.memory_usage_mb)
            )
        } else {
            "CPU --  ·  MEM --".to_string()
        };

        // Health indicator
        let health_icon = self.health_icon(container);

        // Port mappings text
        let ports_text = format_ports(&container.ports);

        // First public port for browser button
        let first_public_port = container
            .ports
            .iter()
            .find_map(|p| p.public_port);

        // Row 1: health + name + action buttons
        let actions: Element<Message> = if is_pending {
            text::caption(fl!("loading")).into()
        } else {
            let mut row = widget::row().spacing(4).align_y(Alignment::Center);

            row = row.push(
                widget::button::icon(widget::icon::from_name(
                    "media-playback-stop-symbolic",
                ))
                .extra_small()
                .tooltip(fl!("stop"))
                .on_press(Message::StopContainer(container.id.clone())),
            );

            row = row.push(
                widget::button::icon(widget::icon::from_name("view-refresh-symbolic"))
                    .extra_small()
                    .tooltip(fl!("restart"))
                    .on_press(Message::RestartContainer(container.id.clone())),
            );

            if let Some(port) = first_public_port {
                row = row.push(
                    widget::button::icon(widget::icon::from_name("web-browser-symbolic"))
                        .extra_small()
                        .tooltip(fl!("open-browser"))
                        .on_press(Message::OpenInBrowser(port)),
                );
            }

            row = row.push(
                widget::button::icon(widget::icon::from_name("edit-copy-symbolic"))
                    .extra_small()
                    .tooltip(fl!("copy-id"))
                    .on_press(Message::CopyContainerId(container.id.clone())),
            );

            row = row.push(
                widget::button::icon(widget::icon::from_name("dialog-information-symbolic"))
                    .extra_small()
                    .tooltip(fl!("details"))
                    .on_press(Message::ShowDetails(
                        container.id.clone(),
                        container.name.clone(),
                    )),
            );

            row = row.push(
                widget::button::icon(widget::icon::from_name(
                    "utilities-terminal-symbolic",
                ))
                .extra_small()
                .tooltip(fl!("logs"))
                .on_press(Message::ShowLogs(
                    container.id.clone(),
                    container.name.clone(),
                )),
            );

            row.into()
        };

        let mut name_row = widget::row()
            .align_y(Alignment::Center)
            .spacing(4);

        if let Some(icon) = health_icon {
            name_row = name_row.push(icon);
        }

        name_row = name_row
            .push(text::body(&container.name).width(Length::Fill))
            .push(actions);

        let mut col = widget::column()
            .push(name_row)
            .push(text::caption(&container.image))
            .spacing(2)
            .padding(8)
            .width(Length::Fill);

        if !ports_text.is_empty() {
            col = col.push(text::caption(ports_text));
        }

        col = col.push(text::caption(stats_text));

        // Uptime / status
        col = col.push(text::caption(&container.status));

        col.into()
    }

    fn view_stopped_container<'a>(
        &'a self,
        container: &'a ContainerInfo,
    ) -> Element<'a, Message> {
        let is_pending = self.pending_ops.contains(&container.id);

        let health_icon = self.health_icon(container);
        let ports_text = format_ports(&container.ports);

        // Check if this container has a pending delete confirmation
        let confirming_delete = self
            .confirm_delete
            .as_ref()
            .map(|id| id == &container.id)
            .unwrap_or(false);

        // Row 1: name + action buttons
        let actions: Element<Message> = if is_pending {
            text::caption(fl!("loading")).into()
        } else if confirming_delete {
            widget::row()
                .push(text::caption(fl!(
                    "confirm-delete",
                    name = container.name.as_str()
                )))
                .push(
                    widget::button::text(fl!("confirm-yes"))
                        .on_press(Message::ConfirmDelete(container.id.clone()))
                        .class(cosmic::theme::Button::Destructive),
                )
                .push(
                    widget::button::text(fl!("confirm-no"))
                        .on_press(Message::CancelDelete)
                        .class(cosmic::theme::Button::Standard),
                )
                .spacing(4)
                .align_y(Alignment::Center)
                .into()
        } else {
            widget::row()
                .push(
                    widget::button::icon(widget::icon::from_name(
                        "media-playback-start-symbolic",
                    ))
                    .extra_small()
                    .tooltip(fl!("start"))
                    .on_press(Message::StartContainer(container.id.clone())),
                )
                .push(
                    widget::button::icon(widget::icon::from_name("user-trash-symbolic"))
                        .extra_small()
                        .tooltip(fl!("delete"))
                        .on_press(Message::DeleteContainer(container.id.clone())),
                )
                .push(
                    widget::button::icon(widget::icon::from_name("edit-copy-symbolic"))
                        .extra_small()
                        .tooltip(fl!("copy-id"))
                        .on_press(Message::CopyContainerId(container.id.clone())),
                )
                .push(
                    widget::button::icon(widget::icon::from_name(
                        "dialog-information-symbolic",
                    ))
                    .extra_small()
                    .tooltip(fl!("details"))
                    .on_press(Message::ShowDetails(
                        container.id.clone(),
                        container.name.clone(),
                    )),
                )
                .push(
                    widget::button::icon(widget::icon::from_name(
                        "utilities-terminal-symbolic",
                    ))
                    .extra_small()
                    .tooltip(fl!("logs"))
                    .on_press(Message::ShowLogs(
                        container.id.clone(),
                        container.name.clone(),
                    )),
                )
                .spacing(4)
                .align_y(Alignment::Center)
                .into()
        };

        let mut name_row = widget::row()
            .align_y(Alignment::Center)
            .spacing(4);

        if let Some(icon) = health_icon {
            name_row = name_row.push(icon);
        }

        name_row = name_row
            .push(text::body(&container.name).width(Length::Fill))
            .push(actions);

        let mut col = widget::column()
            .push(name_row)
            .push(text::caption(&container.image))
            .spacing(2)
            .padding(8)
            .width(Length::Fill);

        if !ports_text.is_empty() {
            col = col.push(text::caption(ports_text));
        }

        // Status
        col = col.push(text::caption(&container.status));

        col.into()
    }

    fn view_logs(&self) -> Element<'_, Message> {
        let header = widget::row()
            .push(
                widget::button::icon(widget::icon::from_name("go-previous-symbolic"))
                    .on_press(Message::BackToList),
            )
            .push(text::title4(&self.log_container_name))
            .align_y(Alignment::Center)
            .spacing(8)
            .padding(8);

        let log_body: Element<Message> = if self.logs_loading && self.log_content.is_empty() {
            widget::container(text::body(fl!("loading")))
                .padding(16)
                .center_x(Length::Fill)
                .into()
        } else {
            let log_text = if self.log_content.is_empty() {
                "(no output)".to_string()
            } else {
                self.log_content.clone()
            };
            scrollable(text::monotext(log_text).width(Length::Fill))
                .height(400)
                .into()
        };

        widget::column()
            .push(header)
            .push(widget::divider::horizontal::light())
            .push(log_body)
            .spacing(4)
            .width(Length::Fill)
            .into()
    }

    fn view_details(&self) -> Element<'_, Message> {
        let header = widget::row()
            .push(
                widget::button::icon(widget::icon::from_name("go-previous-symbolic"))
                    .on_press(Message::BackToList),
            )
            .push(text::title4(&self.details_container_name))
            .align_y(Alignment::Center)
            .spacing(8)
            .padding(8);

        let body: Element<Message> = if self.details_loading {
            widget::container(text::body(fl!("loading")))
                .padding(16)
                .center_x(Length::Fill)
                .into()
        } else if let Some(details) = &self.details_data {
            let mut col = widget::column().spacing(8).padding([0, 12]);

            // Ports section - find the container to get its ports
            let container_ports: Vec<&PortMapping> = self
                .containers
                .iter()
                .find(|c| c.name == self.details_container_name)
                .map(|c| c.ports.iter().collect())
                .unwrap_or_default();

            col = col.push(text::body(fl!("ports")));
            if container_ports.is_empty() {
                col = col.push(text::caption(fl!("no-data")));
            } else {
                for port in &container_ports {
                    let port_str = if let Some(pub_port) = port.public_port {
                        format!("{}:{}/{}", pub_port, port.private_port, port.protocol)
                    } else {
                        format!("{}/{}", port.private_port, port.protocol)
                    };
                    col = col.push(text::caption(port_str));
                }
            }

            col = col.push(widget::divider::horizontal::light());

            // Volumes section
            col = col.push(text::body(fl!("volumes")));
            if details.volumes.is_empty() {
                col = col.push(text::caption(fl!("no-data")));
            } else {
                for (src, dst) in &details.volumes {
                    col = col.push(text::caption(format!("{} → {}", src, dst)));
                }
            }

            col = col.push(widget::divider::horizontal::light());

            // Networks section
            col = col.push(text::body(fl!("networks")));
            if details.networks.is_empty() {
                col = col.push(text::caption(fl!("no-data")));
            } else {
                for (name, ip) in &details.networks {
                    let net_text = if ip.is_empty() {
                        name.clone()
                    } else {
                        format!("{} ({})", name, ip)
                    };
                    col = col.push(text::caption(net_text));
                }
            }

            col = col.push(widget::divider::horizontal::light());

            // Environment Variables section
            col = col.push(text::body(fl!("environment")));
            if details.env_vars.is_empty() {
                col = col.push(text::caption(fl!("no-data")));
            } else {
                for var in &details.env_vars {
                    col = col.push(text::caption(var));
                }
            }

            scrollable(col).height(400).into()
        } else {
            widget::container(text::body(fl!("no-data")))
                .padding(16)
                .center_x(Length::Fill)
                .into()
        };

        widget::column()
            .push(header)
            .push(widget::divider::horizontal::light())
            .push(body)
            .spacing(4)
            .width(Length::Fill)
            .into()
    }

    fn health_icon<'a>(&self, container: &ContainerInfo) -> Option<Element<'a, Message>> {
        let status = self.health.get(&container.id)?;
        let icon_name = match status {
            HealthStatus::Healthy => "emblem-ok-symbolic",
            HealthStatus::Unhealthy => "emblem-important-symbolic",
            HealthStatus::Starting => "emblem-synchronizing-symbolic",
            HealthStatus::None => return None,
        };
        Some(
            widget::icon::from_name(icon_name)
                .size(16)
                .into(),
        )
    }
}

fn format_ports(ports: &[PortMapping]) -> String {
    let mappings: Vec<String> = ports
        .iter()
        .filter_map(|p| {
            p.public_port.map(|pub_port| {
                format!("{}:{}/{}", pub_port, p.private_port, p.protocol)
            })
        })
        .collect();

    if mappings.is_empty() {
        String::new()
    } else {
        mappings.join(", ")
    }
}

fn format_memory(mb: f64) -> String {
    if mb >= 1024.0 {
        format!("{:.1}G", mb / 1024.0)
    } else {
        format!("{:.0}M", mb)
    }
}
