# Identidade visual do Warden

> Primeiro corte (Sessão 288, P137): marca, tokens e o `site/`. Sessão 289: o `web/` também (`web/src/tokens.css` é cópia de `design/tokens.css`, com teste). Sessão 290: o desktop também (`desktop/src/tokens.css`, botão de tema nas Configurações, `LogoMark` com o escudo novo). Sessão 291: a extensão também (`extension/src/sidepanel/tokens.css`, `theme-init.js` em `public/` porque uma página de extensão não aceita script inline, escudo ao lado do título, `icon-128.png` novo). Sessão 292: o mobile também (`mobile/lib/theme/warden_theme.dart` é o `ColorScheme` explícito com os mesmos hex, e `test/theme/warden_theme_test.dart` lê `design/tokens.css` e falha se algum divergir; `AppTheme` guarda `app.theme`, escuro por padrão; `ThemeButton` nas duas barras; `WardenLogo` desenhado à mão ao lado do título; fonte padrão do Android, a Roboto, sem a Inter). Os quatro apps e o site estão na identidade. `--color-accent-dark` no escuro é `#8b5cf6` (19 regras o usam como cor de texto; com `#7c3aed` o contraste caía a ~3:1).
> **Escuro por padrão em toda superfície, sem seguir o sistema**: o claro só com `data-theme="light"` (botão de tema, guardado em `localStorage` como `warden-theme`, a mesma chave do site e do `web/`). `--gradient-button` é o degradê do botão cheio (texto escuro sobre o roxo claro no escuro, branco sobre o roxo escuro no claro).

## O que transmite
Um guardião sóbrio: segurança e controle nas suas mãos. Texto direto e técnico, sem exagero. O produto é **preto**, com **roxo só em detalhes e degradês**; nunca azul nem ciano como cor de marca (o Arven é preto e ciano).

## Marca
- Escudo com um "W" de traço contínuo, em degradê roxo (`#c4b5fd` → `#7c3aed`) sobre `#0d0a18`. Fontes: `site/public/brand/icon.svg` (e `site/app/icon.svg`, o favicon), `icon-512.png`, `og.png` (1200x630) e `site/app/apple-icon.png`; o componente é `site/components/Logo.tsx`.
- Uso: o escudo sozinho em espaços pequenos; o escudo com a palavra "Warden" (Inter 600) no cabeçalho e no rodapé.

## Cores e tokens
Fonte única: `design/tokens.css` (nomes `--color-*` iguais aos de `web/src/App.css` e `desktop/src/App.css`, para eles adotarem sem renomear). O site usa uma cópia em `site/app/tokens.css`; `site/tests/tokens.test.mjs` falha se as duas divergirem.

| Token | Escuro (padrão) | Claro |
|---|---|---|
| `--color-bg` | `#07060b` | `#fbf9ff` |
| `--color-surface` | `#100d1a` | `#ffffff` |
| `--color-border` | `#272043` | `#e3d9f7` |
| `--color-text` | `#ede9f7` | `#1b1330` |
| `--color-text-muted` | `#a599c2` | `#6b6280` |
| `--color-accent` | `#a78bfa` | `#7c3aed` |

Degradês: `--gradient-accent` (botão primário, a palavra em destaque do título) e `--gradient-glow` (névoa roxa no topo das seções). Tema escuro por padrão; o claro vem do sistema ou do botão de tema (`data-theme` no `<html>`, guardado em `localStorage` como `warden-theme`).

## Tipografia
Inter (texto) e JetBrains Mono (código), pelos pacotes `@fontsource-variable/*`, empacotados no build (sem chamada externa).

## Layout (estilo da referência do Arven)
Cabeçalho fixo com vidro, hero com pílula "Open source", botão cheio e botão de contorno, janela de app com borda luminosa, faixa clara de recursos, bloco de instalação com abas e botão de copiar, cartões de vidro, rodapé enxuto, rótulos pequenos em maiúsculas acima dos títulos.

## Mobile
Não importa CSS: `mobile/lib/theme/warden_theme.dart` repete os hex de `design/tokens.css` em `WardenPalette` (escuro e claro) e monta o `ThemeData`; o teste de deriva fica em `mobile/test/theme/warden_theme_test.dart`. Ao mudar um token, mude os dois lugares. As fotos do mobile vêm de um teste de widget descartável (fontes Roboto e MaterialIcons do cache do SDK carregadas à mão), não de um aparelho.

## Falta
Domínio próprio, versão em pt-BR do site e a fonte Inter no mobile (pediria `.ttf`). Feitos na Sessão 293: os ícones de 16, 32 e 48 px da extensão (`extension/public/icon-*.png`, no `icons` e no `action.default_icon` do manifesto) e o ícone adaptativo do Android (`mipmap-anydpi-v26/ic_launcher.xml`, com fundo `#07060B`, o escudo em vetor e uma versão monocromática para os ícones temáticos do Android 13; os PNGs de cada `mipmap-*` continuam para o Android antigo).
