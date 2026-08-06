//! Adaptador MCP/JSON-RPC: gateway, negociación de protocolo, herramientas y errores.
//!
//! Transporte actual: stdio (decisión 2026-08, ver docs/01-journal). El modo
//! HTTP loopback (ADR-006, THR-001) se añadirá cuando exista una necesidad
//! real de multi-cliente.

pub mod jsonrpc;
pub mod server;
