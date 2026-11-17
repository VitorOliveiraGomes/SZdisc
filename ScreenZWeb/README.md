# ScreenZ — Interface

Interface do ScreenZ em React e TypeScript, com Vite, Tailwind CSS e componentes Radix. O pacote se chama `screenz-web`. A versão desktop usa Tauri 2 com o backend Rust na raiz do projeto.

## Desenvolvimento

Execute nesta pasta:

```powershell
npm ci --legacy-peer-deps
npm run desktop:dev
```

Para abrir somente a prévia no navegador, execute `npm run dev`. Sem o backend local, a interface usa dados simulados.

## Compilação e verificações

```powershell
npm run build
npm run lint
npm test
npm run desktop:build
```

O build web gera `dist/`, e o build desktop gera o executável na pasta `target/release/` da raiz. A logo SZ está em `public/favicon.svg`; os ícones nativos estão em `../icons/`.

Consulte o [README do ScreenZ](../README.md) para requisitos, perfil, captura de tela e comandos completos, e a [arquitetura](../docs/ARQUITETURA.md) para a organização do código.
