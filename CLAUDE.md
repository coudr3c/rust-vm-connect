# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust GUI application that creates AWS SSM tunnels to EC2 instances and launches RDP connections. It uses egui for the GUI and manages both SSM sessions and RDP processes concurrently.

## Build Commands

- `cargo build` - Build the project
- `cargo run` - Run the application
- `make build-windows` - Cross-compile for Windows (x86_64-pc-windows-gnu)
- `make build-windows-release` - Cross-compile release build for Windows

## Code Architecture

### Core Components

- **main.rs**: Contains the egui GUI application (`EguiApp`) that manages user input and displays logs
- **tasks_handler.rs**: Orchestrates SSM tunnel creation and RDP connection spawning
- **ssm.rs**: Handles AWS SSM session management and tunnel creation
- **rdp.rs**: Manages RDP process spawning and lifecycle
- **messages.rs**: Defines inter-component communication messages
- **errors.rs**: Centralized error handling types
- **utils.rs**: Shared utilities including logging functions

### Threading Model

The application uses a complex threading model:
1. Main GUI thread runs the egui application
2. A blocking thread spawns a tokio runtime for async AWS operations
3. SSM tunnel runs in a tokio task
4. RDP process runs as a separate system process

### Communication Flow

1. GUI spawns a thread that creates a tokio runtime
2. `tasks_handler::start()` coordinates SSM tunnel and RDP connection
3. SSM tunnel notifies when ready via `SSMTunnelLaunchedMessage`
4. RDP connection spawns after tunnel is established
5. `ApplicationExitedMessage` signals shutdown across all components

### Error Handling

Each component has its own error type (`SSMError`, `RDPError`, `TaskHandlerError`) that flows up through the system. The GUI displays error messages in the logs output.

## Development Notes

- The application targets both Linux and Windows RDP clients
- AWS credentials must be configured in the environment
- Default VM targets are hardcoded in `EguiApp::default()`
- Logging uses a channel-based system for thread-safe communication to the GUI