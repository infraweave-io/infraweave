fn main() {
    println!("cargo:warning=This package requires maturin to build Python artifacts.");
    println!("cargo:warning=Run `maturin develop --release` manually after `cargo build`.");
}
