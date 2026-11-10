# SyncLights

Aplicativo desktop Linux para sincronizar a fita ROBOBLOQ SyncLight (`1a86:fe07`) com as cores nas bordas de um monitor. O motor em Rust captura só as três faixas usadas pelos 65 LEDs; a interface Qt6 fica na bandeja e mantém a captura independente do preview.

## Estado

- X11 com RandR, XFixes opcional para incluir o ponteiro, fita USB testada neste host.
- Captura direta de regiões até 120 FPS; o controle Velocidade só altera a transição temporal entre cenas. Jogo usa uma média estratificada rápida 8×8 por LED, contraste/intensidade fortes e resposta imediata; Filme aplica uma transição suave sem baixar a captura.
- Preview independente, limitado a 10 FPS e 320×180.
- Configurações persistentes, seleção de monitor, inversão da fita, perfis, ícone de bandeja e autostart.
- Acesso hidraw por regra udev instalada uma vez; o app não pede sudo durante o uso.

O hardware validado foi uma fita `1a86:fe07` no X11 de CachyOS. Outros dispositivos/fabricantes e Wayland ainda não foram validados.

## Compilar e instalar no usuário atual

Dependências: Rust/Cargo, CMake, compilador C++20, Qt6 Widgets, FFmpeg com `x11grab`, X11 e `xrandr`.

```sh
cargo test --release --offline
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build -j2
cmake --install build --prefix "$HOME/.local"
```

Inclua `~/.local/bin` no `PATH`. Depois abra SyncLights no menu de aplicativos ou execute `synclights-desktop --open`. A primeira ativação de acesso USB requer uma regra udev administrativa; neste computador ela já está instalada:

```udev
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1a86", ATTRS{idProduct}=="fe07", ATTRS{bInterfaceNumber}=="00", MODE="0660", GROUP="users"
```

Aplicar a regra exige uma única configuração administrativa e reconectar a fita. O uso cotidiano não exige sudo. A interface 01 é teclado/botões e não é aberta pelo motor.

## Diagnóstico e testes

```sh
synclights probe /dev/hidraw7
synclights bench-roi 0
synclights bench-algorithms 0
synclights sync auto 0 120 box normal 1.5 1.2
```

Estimadores disponíveis: grade 8×8 (padrão rápido), média integral da área (`box`), Gaussiano 3×3, mediana 3×3, luminância ponderada, pixel único (diagnóstico) e redução FFmpeg 320×180. O caminho `resize` tem limite de quadros explícito; seu maior custo é capturar e reduzir a tela inteira.

O painel mostra o tempo software frame→escrita USB (P95), não a latência óptica da fita. A confirmação física dessa última exige sensor de luz/câmera de alta velocidade. Resultados medidos neste computador estão em [`docs/benchmarks.md`](docs/benchmarks.md).

## Licença e site

MIT. Site estático em [`docs/index.html`](docs/index.html), publicado no GitHub Pages sem Actions.
