# Medições de performance

Medições feitas em 10 nov 2026 em CachyOS/X11, dois monitores 1920×1080, DP-0 selecionado, fita ROBOBLOQ USB `1a86:fe07` conectada. O cursor foi movimentado sobre a faixa esquerda durante os testes dinâmicos. Resultados dependem da carga simultânea da máquina.

## Captura e tráfego

| Caminho | Captura medida | Perdas | Dados lidos por quadro | Observação |
|---|---:|---:|---:|---|
| X11 GetImage em três ROIs | P50 0,70 ms · P95 1,24 ms · P99 1,40 ms (bench sem limite) | — | 1,01 MB | Faixa superior 1920×63 e laterais 61×1080 |
| X11-ROI em 120 Hz, método grid | 117,5–117,9 FPS | 0 | ~1,01 MB | P95 software 7,1–7,5 ms enquanto o cursor mudava as cores da fita |
| FFmpeg resize 320×180 em 120 Hz | 112,2–119,7 FPS | 0 | ~8,29 MB de entrada | P95 software 5,9–6,1 ms no teste dinâmico; usa cerca de 8,2× mais banda de captura |
| Preview FFmpeg | limitado a 6 FPS, 320×180 | slot de última imagem | 0,23 MB por quadro | Independente do cálculo dos LEDs |

O primeiro teste de resize revelou que `scale` sozinho permitia à saída raw repetir milhares de quadros por segundo. Adicionamos `fps=120` antes do redimensionamento e medimos novamente; o contador caiu para ~119 FPS sem perdas. O preview recebe o mesmo limite explícito (`fps=6`).

DP-3 (monitor 1) também foi exercitado com a fita e inversão: captura ~116 FPS sem perda no loop, amostragem grid 0,10 ms, escrita HID ~6,0 ms. O bench de captura nesse monitor ficou P50 1,90 / P95 2,79 / P99 4,38 ms, mais lento que DP-0 nesta sessão.

## Estimadores

250 iterações de 65 LEDs sobre as faixas X11 capturadas ao vivo. `MAE-vs-box` é o erro médio absoluto por canal (0–255) contra a média integral naquela imagem de desktop; ele caracteriza apenas o frame observado, não todo jogo/filme.

| Estimador | Custo dos 65 LEDs | MAE vs média integral | Uso |
|---|---:|---:|---|
| Média estratificada 8×8 (`grid`) | 41 µs/quadro | 3,62 | Padrão de Jogo: ~43× mais rápido que a média integral neste ensaio |
| Média integral da área (`box`) | 1.764 µs/quadro | 0 | Referência de cor mais estável; maior custo |
| Gaussiano 3×3 | 4,6 µs/quadro | 23,17 | Reage aos pixels locais e suaviza o ponto amostrado |
| Mediana 3×3 | 5,4 µs/quadro | 21,82 | Resiste a um único pixel extremo |
| Peso por luminância (`luma`) | 2.373 µs/quadro | 12,31 | Considera a luminância de toda a área; alto custo |
| Pixel central (`nearest`) | 0,7 µs/quadro | 24,76 | Diagnóstico; pode errar a cor de cenas com detalhe localizado |
| Redução FFmpeg (`resize`) | 0,012 ms para os 65 LEDs | medido à parte | Custo dominante é ler a tela inteira e reduzi-la |

No mesmo frame, `grid` ficou a 0,06 ms por quadro e a média integral a 2,24 ms durante o teste no HID. O erro da grade 8×8 foi pequeno nesse frame; o teste unitário de gradientes lineares limita sua diferença de média a 3 valores por canal. Em imagens com padrões menores que a grade, os resultados podem divergir mais; por isso a média integral continua selecionável.

## Escrita HID e latência

Com mudança de cor no HID conectado, os estimadores diretos mantiveram ~115–118 FPS sem perda. P95 software frame→fim da escrita: `grid` 7,1–7,5 ms; `box` 10,6–12,7 ms; Gaussian/mediana/pixel central 6,3–7,3 ms; luminância 9,5–9,6 ms. O resize ficou em 5,9–6,1 ms no teste dinâmico. As escritas HID levaram em média ~5,2–5,8 ms.

Esse cronômetro termina quando o `write` USB do host retorna. Ele **não** mede o processamento interno da controladora nem o tempo até os LEDs emitirem luz; para essa latência física é necessário um fotodiodo ou câmera de alta velocidade. O app mostra FPS real e deixa P95 como “—” quando a tela não muda e nenhum pacote é enviado.

Reproduza com `synclights bench-roi 0`, `synclights bench-algorithms 0` e, com a fita ligada, `synclights sync auto 0 120 grid normal 1.5 1.2`. Métodos: Rust (captura/amostragem/protocolo), x11rb RandR + XFixes, Qt6 Widgets, FFmpeg x11grab/scale, libc (sinal de encerramento do subprocesso).
