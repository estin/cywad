pub fn new() -> impl cywad_core::EngineTrait {
    #[cfg(feature = "webkit")]
    let engine = cywad_webkit::Webkit {};

    #[cfg(feature = "devtools")]
    let engine = cywad_devtools::Devtools {};

    engine
}
