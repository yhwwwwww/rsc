use std::io::{self, Read};
fn main() {
    let args:Vec<String>=std::env::args().skip(1).collect();
    println!("{:?}",args);
    if args.iter().any(|s|s=="--stdin") {
        let mut s=String::new();io::stdin().read_to_string(&mut s).unwrap();print!("{s}");
    }
    if args.iter().any(|s|s=="--exit-42") {std::process::exit(42);}
}
