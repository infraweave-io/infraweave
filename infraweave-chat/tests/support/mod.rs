//! Shared test scaffolding for the chat eval suite.
//!
//! Integration tests each compile as their own crate, so every test file that
//! needs this declares `mod support;`. Items unused by a given test file are
//! expected - hence the allow below.
#![allow(dead_code)]

pub mod fake_llm;
pub mod fixtures;
pub mod scenario;
pub mod stub_api;
