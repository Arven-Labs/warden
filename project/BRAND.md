# Identidade visual do Warden

> Primeiro corte (Sessão 288, P137): marca, tokens e o `site/`. Sessão 289: o `web/` também (`web/src/tokens.css` é cópia de `design/tokens.css`, com teste). Desktop, extensão e mobile ainda não.
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

## Mobile (anotado, não aplicado)
`mobile/lib/main.dart` usa `ColorScheme.fromSeed` com `0xFF7C3AED` (claro) e `0xFFA78BFA` (escuro). Para a identidade nova: `surface` `0xFF07060B`, `onSurface` `0xFFEDE9F7`, `primary` `0xFFA78BFA`, `outline` `0xFF272043`, tema escuro por padrão.

## Falta
Desktop, extensão e mobile adotarem os tokens (um por sessão, com fotos de E2E antes e depois), domínio próprio, versão em pt-BR do site.
