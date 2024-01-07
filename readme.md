
collect simple custom stats on my openwrt based router(turris omnia with armv7 cpu)
binary is cross-compiled on mac using following procedure:


prepare environment:
```
rustup target add armv7-unknown-linux-musleabihf
brew install arm-linux-gnueabihf-binutils
```


compile (see .cargo/config.toml for setup):
```
cargo build
```
