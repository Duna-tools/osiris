---
name: Pull request
description: Contribución a Osiris
title: "[PR] "
labels: []
body:
  - type: markdown
    attributes:
      value: "Antes de abrir, revisa [CONTRIBUTING.md](../blob/main/CONTRIBUTING.md). El guantelete (fmt, clippy -D warnings, tests) debe pasar."
  - type: textarea
    id: summary
    attributes:
      label: Qué resuelve
      description: "Resumen del cambio y problema que ataca"
    validations:
      required: true
  - type: textarea
    id: verification
    attributes:
      label: Cómo se verificó
      description: "Tests, verificación contra Godot real, contract tests"
    validations:
      required: true
  - type: checkboxes
    id: checklist
    attributes:
      label: Checklist
      options:
        - label: "`cargo fmt --check` pasa"
        - label: "`cargo clippy --workspace --all-targets -- -D warnings` pasa"
        - label: "`cargo test --workspace` pasa"
        - label: "Si toqué el bridge GDScript: verificado contra Godot real (4.7+)"
        - label: "Tools nuevas: ToolSpec + schema MCP + op en bridge + contract test"
