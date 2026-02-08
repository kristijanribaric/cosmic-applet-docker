# cosmic-applet-docker

A panel applet for the [COSMIC desktop](https://github.com/pop-os/cosmic-epoch) that lets you monitor and manage Docker containers without leaving your workflow.


![Screenshot](./screenshots/demo1.png)


## Features

- **Panel indicator** with running container count
- **Live stats** — CPU and memory usage per container, updated every 3 seconds
- **Quick actions** — start, stop, restart containers with one click
- **Log viewer** — tail the last 100 lines of any container's logs
- **Grouped layout** — running containers on top, stopped containers below

## Requirements

- COSMIC desktop environment
- Docker daemon running and accessible to the current user (i.e. user in the `docker` group, or rootless Docker)
- Rust toolchain and [just](https://github.com/casey/just) (to build from source)

## Install

```sh
git clone https://github.com/youruser/cosmic-applet-docker.git
cd cosmic-applet-docker
just build
sudo just install
```

Then add the applet to your panel through COSMIC Settings > Desktop > Panel > Applets.

To uninstall:

```sh
sudo just uninstall
```

## Usage

Click the applet icon in the panel to open the popup. Running containers show live CPU/memory stats and action buttons (stop, restart, logs). Stopped containers show their exit status and can be started or inspected.

<!-- TODO: Add screenshots of the popup in different states
### Running containers
![Running](./screenshots/running.png)

### Stopped containers
![Stopped](./screenshots/stopped.png)

### Log viewer
![Logs](./screenshots/logs.png)
-->

## License

GPL-3.0
