build-windows:
	cargo build --target x86_64-pc-windows-gnu

build-windows-release:
	cargo build --release --target x86_64-pc-windows-gnu