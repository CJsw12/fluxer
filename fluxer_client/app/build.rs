//! Embeds the official Fluxer icon and a VERSIONINFO resource into the exe.
//!
//! `embed-resource` is already in the workspace lockfile (it is a build
//! dependency of gpui-pre), so no additional crate is introduced. The resource
//! file intentionally contains no RT_MANIFEST because gpui-pre already embeds
//! one; a second manifest fails the link with CVT1100.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/app.rc");
    println!("cargo:rerun-if-changed=assets/icon/icon.ico");

    // GPUI element trees and debug frames can exceed the 1 MiB MSVC default main-thread stack.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bin=fluxer_client=/STACK:8388608");
    }

    // `manifest_required` makes a missing rc.exe a build error instead of
    // silently producing an exe without icon or version information.
    if let Err(err) = embed_resource::compile("assets/icon/app.rc", embed_resource::NONE).manifest_required() {
        panic!("could not embed the Windows resources: {err}");
    }
}
