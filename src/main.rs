use cosmic_applet_docker::DockerApplet;

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt::init();
    cosmic::applet::run::<DockerApplet>(())
}
