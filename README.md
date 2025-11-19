# TEST

```bash
# build
docker run --rm -it -p 9100:9100 -v $(pwd):/mnt/src rust
cd /mnt/src
cargo build

# cpu throtling
docker run --rm -it -p 9100:9100 -v $(pwd):/mnt/src --cpus=0.5 rust
cd /mnt/src
yes > /dev/null &
TARGET_PID=8 METRICS_PREFIX=docker ./target/debug/cgroup-exporter
RUST_LOG="info,hyper=warn" TARGET_PID=8 METRICS_PREFIX=docker ./target/debug/cgroup-exporter
```
