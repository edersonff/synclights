# SyncLights · checklist de entrega

## Feito

- [x] Identificar o HID ROBOBLOQ `1a86:fe07`; interface 00 controla LEDs e 01 é teclado.
- [x] Corrigir o pacote de frame (contador correto, ID e tamanho) e manter o descritor HID aberto durante a sessão.
- [x] Regra udev `0660:users` para usar a fita sem sudo em cada execução.
- [x] Capturar somente três ROIs de borda por X11; incluir cursor com XFixes quando disponível.
- [x] Limitar FPS do motor a 120 e limitar FFmpeg explicitamente com filtro `fps`, evitando milhares de frames duplicados.
- [x] Manter preview independente a 320×180 e 10 FPS.
- [x] Modos Jogo, Filme e Ambiente; persistência de monitor, método, controles, inversão, autostart e tamanho da janela.
- [x] Bandeja, ícone e instalação no diretório do usuário.
- [x] Cobrir protocolo, sequência de pacote e equivalência dos cinco amostradores com testes automatizados.
- [x] Fazer testes reais no DP-0 e na fita conectada, com ponteiro em movimento; registrar método, FPS, perda e P95.
- [x] Publicar instruções, licença MIT e site estático.

## Limites conhecidos

- A medida frame→HID termina quando a escrita USB retorna; não mede o tempo interno da controladora nem a resposta óptica dos LEDs.
- A validação de hardware e tela ocorreu em X11 no CachyOS, com DP-0/DP-3 e fita USB `1a86:fe07`. Wayland e outras fitas aguardam validação.
- A estratégia resize FFmpeg lê a tela inteira antes da redução e usa mais banda/CPU que as ROIs diretas; permanece como comparador opcional.
- LED físico realimentado (variação de cor distinta em tela, incluindo uma cena de jogo/filme) precisa de confirmação visual pelo dono. Cursor e atualização HID foram exercitados na fita durante os benchmarks.

## Próximos itens

- [ ] Testar visualmente o app depois da instalação limpa e confirmar transições em jogo e filme.
- [ ] Fazer teste sustentado longo (30–60 min) e observar temperatura, consumo e estabilidade de conexão USB.
- [ ] Obter medição óptica com fotodiodo/câmera para caracterizar a latência total tela→LED.
- [ ] Validar XFixes desligado, outros layouts de monitor, desconexão/reconexão e Wayland quando houver ambiente disponível.
- [ ] Avaliar build/appimage para distros sem Rust/Qt já instalados.
