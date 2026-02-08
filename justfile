name := "cosmic-applet-docker"
appid := "com.example.CosmicAppletDocker"

prefix := "/usr"
bindir := prefix / "bin"
appdir := prefix / "share" / "applications"
icondir := prefix / "share" / "icons" / "hicolor" / "scalable" / "apps"

# Build in release mode
build:
    cargo build --release

# Install binary, desktop entry, and icon
install:
    install -Dm755 target/release/{{name}} {{bindir}}/{{name}}
    install -Dm644 data/{{appid}}.desktop {{appdir}}/{{appid}}.desktop
    install -Dm644 data/icons/{{name}}-symbolic.svg {{icondir}}/{{name}}-symbolic.svg

# Remove installed files
uninstall:
    rm -f {{bindir}}/{{name}}
    rm -f {{appdir}}/{{appid}}.desktop
    rm -f {{icondir}}/{{name}}-symbolic.svg

# Build and install
setup: build install

# Remove build artifacts
clean:
    cargo clean
