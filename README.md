# TEST

```bash
# build
docker run --rm -it -p 9100:9100 -v $(pwd):/mnt/src rust
# cpu throtling
docker run --rm -it -p 9100:9100 -v $(pwd):/mnt/src --cpus=0.5 rust
```
