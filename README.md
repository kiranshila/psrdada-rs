# psrdada-rs

[![license](https://img.shields.io/badge/license-Apache--2.0_OR_MIT-blue?style=flat-squaure)](#license)
[![docs](https://img.shields.io/docsrs/psrdada-rs?logo=rust&style=flat-square)](https://docs.rs/psrdada-rs/latest/psrdada-rs/index.html)
[![rustc](https://img.shields.io/badge/rustc-1.57+-blue?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![build status](https://img.shields.io/github/workflow/status/GReX-Telescope/psrdada-rs/CI/main?style=flat-square&logo=github)](https://github.com/GReX-Telescope/psrdada-rs/actions)
[![Codecov](https://img.shields.io/codecov/c/github/GReX-Telescope/psrdada-rs?style=flat-square)](https://app.codecov.io/gh/GReX-Telescope/psrdada-rs)

This is a rust library around the [psrdada](http://psrdada.sourceforge.net/) library commonly used in radio astronomy.
Unfortunately, the C library is for the most part undocumented, so the behavior presented by this rust library is what
the authors have been able to ascertain by reading the original example code.
As such, this might not be a 1-to-1 implementation of the original use case.
