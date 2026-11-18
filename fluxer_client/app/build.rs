//! Embeds the official Fluxer icon and a VERSIONINFO resource into the exe.
//!
//! `embed-resource` is already in the workspace lockfile (it is a build
//! dependency of gpui-pre), so no additional crate is introduced. The resource
//! file intentionally contains no RT_MANIFEST because gpui-pre already embeds
//! one; a second manifest fails the link with CVT1100.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/app.rc");
    println!("cargo:rerun-if-changed=assets/icon/icon.ico");

    // `manifest_required` makes a missing rc.exe a build error instead of
    // silently producing an exe without icon or version information.
    if let Err(err) = embed_resource::compile("assets/icon/app.rc", embed_resource::NONE).manifest_required() {
        panic!("could not embed the Windows resources: {err}");
    }
}
