pub fn quantity(nb: usize) -> String {
    let mut s = String::new();
    let i_str = nb.to_string();
    for (idx, val) in i_str.chars().rev().enumerate() {
        if idx != 0 && idx % 3 == 0 {
            s.insert(0, '_');
        }
        s.insert(0, val);
    }
    s
}
