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
