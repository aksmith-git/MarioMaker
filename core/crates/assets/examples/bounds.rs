fn main() {
    for p in std::env::args().skip(1) {
        let bytes = std::fs::read(&p).unwrap();
        let f = mmk_assets::mesh::MeshFile::parse(&bytes).unwrap();
        for m in &f.models {
            let b = m.bounds;
            println!(
                "{p}: {} w {:.1} h {:.1} d {:.1}",
                m.name,
                b[3] - b[0],
                b[4] - b[1],
                b[5] - b[2]
            );
        }
    }
}
