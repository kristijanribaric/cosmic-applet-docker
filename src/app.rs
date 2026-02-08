use crate::config::APP_ID;
use crate::docker::{self, ContainerInfo, ContainerState, ContainerStats, DockerEvent};
use crate::fl;
use cosmic::app::Core;
use cosmic::iced::platform_specific::shell::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Length, Limits, Subscription};
use cosmic::iced_runtime::core::window;
use cosmic::widget::{self, scrollable, text};
use cosmic::{Action, Element, Task};
use std::collections::{HashMap, HashSet};

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
    LogsReceived(Result<(String, String), String>),
    BackToList,
}

#[derive(Debug, Clone, PartialEq)]
enum PopupView {
    ContainerList,
    ContainerLogs,
}

pub struct DockerApplet {
    core: Core,
    popup: Option<Id>,
    docker_available: bool,
    containers: Vec<ContainerInfo>,
    stats: HashMap<String, ContainerStats>,
    current_view: PopupView,
    log_container_name: String,
    log_content: String,
    logs_loading: bool,
    pending_ops: HashSet<String>,
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
            log_content: String::new(),
            logs_loading: false,
            pending_ops: HashSet::new(),
        };
        (applet, Task::none())
    }

    fn update(&mut self, message: Self::Message) -> Task<Action<Self::Message>> {
        match message {
            Message::TogglePopup => {
                return if let Some(popup_id) = self.popup.take() {
                    self.current_view = PopupView::ContainerList;
                    self.log_content.clear();
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
            },

            Message::StartContainer(id) => {
                self.pending_ops.insert(id.clone());
                return cosmic::task::future(async move {
                    Message::ActionCompleted(docker::start_container(id).await)
                });
            }

            Message::StopContainer(id) => {
                self.pending_ops.insert(id.clone());
                return cosmic::task::future(async move {
                    Message::ActionCompleted(docker::stop_container(id).await)
                });
            }

            Message::RestartContainer(id) => {
                self.pending_ops.insert(id.clone());
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
                self.log_content.clear();
                self.logs_loading = true;
                return cosmic::task::future(async move {
                    Message::LogsReceived(docker::fetch_logs(id).await)
                });
            }

            Message::LogsReceived(result) => {
                self.logs_loading = false;
                match result {
                    Ok((_id, logs)) => {
                        self.log_content = logs;
                    }
                    Err(e) => {
                        self.log_content = format!("Error fetching logs: {}", e);
                    }
                }
            }

            Message::BackToList => {
                self.current_view = PopupView::ContainerList;
                self.log_content.clear();
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
            docker::container_list_subscription(popup_open).map(Message::DockerEvent)
        ];

        if popup_open && self.current_view == PopupView::ContainerList {
            let running_ids: Vec<String> = self
                .containers
                .iter()
                .filter(|c| c.state == ContainerState::Running)
                .map(|c| c.id.clone())
                .collect();

            subs.push(
                docker::container_stats_subscription(running_ids).map(Message::DockerEvent),
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

        if self.containers.is_empty() {
            content = content.push(
                widget::container(text::body(fl!("no-containers")))
                    .padding(16)
                    .width(Length::Fill)
                    .center_x(Length::Fill),
            );
            return scrollable(content).height(Length::Shrink).into();
        }

        // Running containers
        let running: Vec<&ContainerInfo> = self
            .containers
            .iter()
            .filter(|c| c.state == ContainerState::Running)
            .collect();

        for container in &running {
            content = content.push(self.view_running_container(container));
            content = content.push(widget::divider::horizontal::light());
        }

        // Stopped containers
        let stopped: Vec<&ContainerInfo> = self
            .containers
            .iter()
            .filter(|c| c.state != ContainerState::Running)
            .collect();

        if !stopped.is_empty() {
            let stopped_header = widget::row()
                .push(text::caption(format!(
                    "{} ({})",
                    fl!("stopped"),
                    stopped.len()
                )))
                .padding([4, 8]);
            content = content.push(stopped_header);
            content = content.push(widget::divider::horizontal::light());

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

        // Row 1: name + action buttons
        let actions: Element<Message> = if is_pending {
            text::caption(fl!("loading")).into()
        } else {
            widget::row()
                .push(
                    widget::button::icon(widget::icon::from_name(
                        "media-playback-stop-symbolic",
                    ))
                    .extra_small()
                    .tooltip(fl!("stop"))
                    .on_press(Message::StopContainer(container.id.clone())),
                )
                .push(
                    widget::button::icon(widget::icon::from_name("view-refresh-symbolic"))
                        .extra_small()
                        .tooltip(fl!("restart"))
                        .on_press(Message::RestartContainer(container.id.clone())),
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

        let name_row = widget::row()
            .push(text::body(&container.name).width(Length::Fill))
            .push(actions)
            .align_y(Alignment::Center)
            .spacing(8);

        // Row 2: image
        let image_row = text::caption(&container.image);

        // Row 3: stats
        let stats_row = text::caption(stats_text);

        widget::column()
            .push(name_row)
            .push(image_row)
            .push(stats_row)
            .spacing(2)
            .padding(8)
            .width(Length::Fill)
            .into()
    }

    fn view_stopped_container<'a>(&'a self, container: &'a ContainerInfo) -> Element<'a, Message> {
        let is_pending = self.pending_ops.contains(&container.id);

        // Row 1: name + action buttons
        let actions: Element<Message> = if is_pending {
            text::caption(fl!("loading")).into()
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

        let name_row = widget::row()
            .push(text::body(&container.name).width(Length::Fill))
            .push(actions)
            .align_y(Alignment::Center)
            .spacing(8);

        // Row 2: image
        let image_row = text::caption(&container.image);

        // Row 3: status
        let status_row = text::caption(&container.status);

        widget::column()
            .push(name_row)
            .push(image_row)
            .push(status_row)
            .spacing(2)
            .padding(8)
            .width(Length::Fill)
            .into()
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

        let log_body: Element<Message> = if self.logs_loading {
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
            scrollable(
                text::monotext(log_text).width(Length::Fill),
            )
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
}

fn format_memory(mb: f64) -> String {
    if mb >= 1024.0 {
        format!("{:.1}G", mb / 1024.0)
    } else {
        format!("{:.0}M", mb)
    }
}
