# DISC2 — Transmissão de Tela Leve (1080p 60fps)

Aplicativo desktop leve para Windows focado em transmissão de tela e jogos em **1080p 60fps** (com opção de redução em tempo real para `720p`, `480p`, `360p` e `30/15 FPS`), funcionando direto pelo aplicativo estilo Discord, sem precisar de navegador.

## Recursos

- **Qualidade ajustável em tempo real**: alterne entre `1080p`, `720p`, `480p` e `360p`, `60/30/15 FPS` e bitrate (`1.8 Mbps` a `10 Mbps`) mesmo durante a transmissão.
- **Captura de Tela Inteira ou Janela/Jogo**: seletor visual com miniaturas ao vivo + captura nativa de áudio do sistema Windows (`Loopback`).
- **Salas App-para-App**:
  - **Código de 4 dígitos (P2P Internet)**: crie uma sala e compartilhe o código para amigos entrarem direto pelo app.
  - **Descoberta Automática na Rede Local / VPN (LAN, Radmin, ZeroTier)**: salas na mesma rede aparecem automaticamente na barra lateral.
- **Modo Jogo Leve**: oculte a prévia local enquanto joga para economizar GPU sem interromper a transmissão para quem assiste.

## Como Executar

1. Instale as dependências:
   ```bash
   npm install
   ```
2. Inicie o aplicativo:
   - Dê dois cliques em `Iniciar DISC2.bat`, ou execute:
   ```bash
   npm start
   ```
