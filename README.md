# ZenithBar ⚡

> Barra lateral ultra-leve e moderna de diagnósticos e telemetria de hardware para Windows 10/11 escrita em Rust.

Inspirado no consagrado [SidebarDiagnostics](https://github.com/ArcadeRenegade/SidebarDiagnostics), o **ZenithBar** foi reconstruído do zero com foco em:
* **Consumo de memória ínfimo**: ~15 MB a 25 MB de RAM (vs. 100-200 MB+ do original em C#/WPF).
* **Zero impacto na CPU**: < 0.1% em repouso graças ao modelo retained-mode e timers otimizados.
* **Modo Híbrido de Privilégios**: Inicialização instantânea sem prompt chato de UAC para monitoramento cotidiano, com ativação de sensores profundos caso executado como Administrador.
* **Estética Fluent / Dark Glass**: Integração com efeitos Mica e Acrylic nativos do Windows 11/10 via DWM.
* **Windows AppBar Nativo**: Acoplamento à lateral da tela reservando a área útil de trabalho para que janelas maximizadas não o sobreponham, com alternância rápida para modo Overlay flutuante.

---

## 🚀 Compilação

### Requisitos
* Rust 1.80+ (com toolchain `x86_64-pc-windows-gnu` ou `x86_64-pc-windows-msvc`).

### Compilar para Windows
```bash
cargo build --release --target x86_64-pc-windows-gnu
```
O executável final estará disponível em:
`target/x86_64-pc-windows-gnu/release/ZenithBar.exe`

---

## ⚙️ Configuração

O arquivo `config.toml` pode residir junto ao executável (modo portável) ou em `%APPDATA%\ZenithBar\config.toml`:

```toml
refresh_interval_ms = 1000
dock_right = true
use_appbar = true
bar_width = 300
show_gpu = true
show_sparklines = true
```

---

## ⚖️ Licença
Distribuído sob licença MIT / Apache 2.0.
