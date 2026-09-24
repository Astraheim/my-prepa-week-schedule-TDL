// Intègre PrepaWeek_icon.ico comme icône de l'exécutable Windows.
//
// Ne fait rien en dehors d'une compilation pour Windows (grâce à
// #[cfg(windows)] sur la fonction elle-même, pas sur un simple `if` : le
// corps n'est même pas compilé sur les autres plateformes, donc la
// dépendance `winresource` — qui n'est ajoutée que pour la cible Windows
// dans Cargo.toml — n'a pas besoin d'être présente ailleurs).
//
// v1.3 : remplace `winres` (abandonné, et peu fiable pour localiser
// rc.exe sur certaines configurations comme Windows sur ARM64) par
// `winresource`, un fork maintenu avec la même API.

fn main() {
    #[cfg(windows)]
    embed_icon();
}

#[cfg(windows)]
fn embed_icon() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("PrepaWeek_icon.ico");
    if let Err(e) = res.compile() {
        // On n'échoue pas le build pour autant : l'appli doit quand même
        // pouvoir compiler et tourner sans icône personnalisée si jamais
        // l'outil de compilation de ressources Windows (rc.exe, fourni
        // avec les Build Tools / le SDK Windows) n'est pas trouvé.
        println!("cargo:warning=Impossible d'intégrer l'icône ({e}) — le .exe sera généré sans icône personnalisée.");
    }
}
