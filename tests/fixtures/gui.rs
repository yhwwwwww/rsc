#![windows_subsystem = "windows"]
fn main() {
    let args=std::env::args().skip(1).collect::<Vec<_>>();
    if let Some(path)=args.first(){std::fs::write(path,"native gui shim executed").unwrap();}
}
