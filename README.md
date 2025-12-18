# vm-connect

A GUI application that creates AWS SSM tunnels to EC2 instances and launches RDP connections.

## What it does

This application simplifies connecting to AWS EC2 instances via Remote Desktop Protocol (RDP) by:
- Creating an AWS Systems Manager (SSM) tunnel to the target EC2 instance
- Automatically launching an RDP client with the appropriate configuration
- Managing the lifecycle of both the tunnel and RDP connection

## Prerequisites

- Rust toolchain (for building)
- AWS credentials configured in your environment (AWS CLI or environment variables)
- RDP client installed:
  - Windows: Built-in Remote Desktop Connection
  - Linux: xfreerdp or similar
- At least one `.rdp` configuration file in the working directory

## Running the application

```bash
cargo run
```

The application will:
1. Scan the current directory for `.rdp` files
2. Launch a GUI window
3. Allow you to select a VM and RDP configuration
4. Connect via SSM tunnel when you click "Connection"

## Compiling

### Local build
```bash
cargo build
```

### Release build
```bash
cargo build --release
```

### Cross-compile for Windows
```bash
make build-windows          # Debug build
make build-windows-release  # Release build
```

## Usage

1. Place your `.rdp` configuration files in the same directory as the executable
2. Launch the application
3. Select your target VM (VM 1, VM 2, or VM 3)
4. Choose an RDP configuration file from the dropdown
5. Optionally modify the local port (default: 55678)
6. Click "Connection" to establish the tunnel and launch RDP

The application will automatically:
- Create an SSM tunnel to the selected EC2 instance
- Launch your RDP client with the selected configuration
- Clean up the tunnel when you close the RDP connection or the application

## Configuration

VM instance IDs are configured in `src/main.rs`:
- VM 1: `i-0f30a1dd89600b0dc`
- VM 2: `i-0a6eb481a98d54b72`
- VM 3: `i-03a933321d29f9f95`

To use different instances, modify these constants in the source code and rebuild.
