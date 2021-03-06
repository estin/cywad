// use cfg_if::cfg_if;

#[cfg(all(feature = "webkit", feature = "devtools"))]
compile_error!("features `crate/webkit` and `crate/devtools` are mutually exclusive");

// cfg_if! {
//     if #[cfg(feature = "webkit")] {
//         extern crate cairo;
//         extern crate gtk;
//         extern crate webkit2gtk;
//     }
// }
//
// cfg_if! {
//     if #[cfg(feature = "devtools")] {
//         extern crate cywad_devtools;
//     }
// }
//
// cfg_if! {
//     if #[cfg(any(feature = "devtools", feature = "server"))] {
//         extern crate futures;
//         extern crate actix;
//         extern crate actix_web;
//         extern crate actix_service;
//     }
// }
//
// cfg_if! {
//     if #[cfg(feature = "server")] {
//         extern crate bytes;
//         extern crate cron;
//         extern crate http;
//         extern crate tokio;
//         extern crate regex;
//         extern crate actix_files;
//         extern crate actix_cors;
//     }
// }
//
// cfg_if! {
//     if #[cfg(feature = "png_widget")] {
//         extern crate image;
//         extern crate imageproc;
//         extern crate rusttype;
//         extern crate hex;
//
//         extern crate lazy_static;
//     }
// }

pub mod engine;
