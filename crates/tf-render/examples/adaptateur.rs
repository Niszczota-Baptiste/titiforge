fn main() {
    match tf_render::Appareil::ouvrir() {
        Ok(a) => {
            println!("adaptateur : {}", a.decrire());
            let l = a.adaptateur.limits();
            println!(
                "  tampon max         : {} Mo",
                l.max_buffer_size / 1_000_000
            );
            println!("  texture 2D max     : {}", l.max_texture_dimension_2d);
            println!("  couches de tableau : {}", l.max_texture_array_layers);
            println!("  groupes de liaison : {}", l.max_bind_groups);
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
