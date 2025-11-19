# CPU load

```bash

cat > cpu_hog.rs << 'EOF'
fn main() {
    let mut x: f64 = 0.0;
    loop {
        // čistě CPU v user-space
        x += (x + 1.0).sin().cos().tan();
        if x > 1e9 {
            x = 0.0;
        }
    }
}
EOF

rustc cpu_hog.rs -O -o cpu_hog
./cpu_hog &
echo $!   # PID

```

# OOM Kill

```bash

cat > memhog.rs << 'EOF'
use std::{env, thread, time::Duration};

fn main() {
    let mb: usize = env::args().nth(1).unwrap_or("300".into()).parse().unwrap();
    let bytes = mb * 1024 * 1024;
    let mut v = Vec::<u8>::with_capacity(bytes);
    v.resize(bytes, 0u8);
    println!("Allocated {} MiB, sleeping...", mb);
    thread::sleep(Duration::from_secs(300));
}
EOF

rustc memhog.rs -O -o memhog
./memhog 300

```
