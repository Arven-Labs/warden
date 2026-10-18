# Diretriz de código (IMPORTANTE — sempre seguir)

**Todo código novo deve ser escrito em inglês — sem exceção.**
- Strings visíveis ao usuário (UI, mensagens de erro, labels, placeholders): inglês
- Nomes de variáveis, funções, classes, arquivos: inglês
- Comentários no código: podem ficar em português (não são visíveis ao usuário e facilitam o aprendizado)
- Esta regra vale para todos os arquivos: `.tsx`, `.ts`, `.rs`

**I18n (múltiplos idiomas) está planejado para uma fase futura:**
Hoje o app é 100% inglês. Quando houver demanda, a estratégia é extrair todas as strings visíveis para arquivos de tradução.

---

# Diretriz de ensino (IMPORTANTE — ler antes de cada sessão)

O usuário é um desenvolvedor experiente (Rust, TypeScript, React, Python, Ruby, Solidity). O objetivo do projeto é construir um sistema funcional e bem arquitetado, não aprender conceitos básicos.

**Regras para o Claude:**
- Explicar decisões de arquitetura antes de implementar
- Oferecer opções com trade-offs quando houver decisão em aberto
- Código limpo, bem tipado, testado
- Manter consistência com a stack já usada nos outros projetos (TruthID, Practice Valuation)

---

# Diretriz de verificação (IMPORTANTE — vale para toda fatia)

O Fabio prefere verificação por código a conferência manual (Sessão 261). Conferir telas e disparar passos de um fluxo são trabalho do
Claude, não dele: o E2E fotografa as telas, o CI sobe as fotos sempre, e o Claude baixa e olha cada uma; o que é fluxo vira automação com
revisão. Tudo no CI, nada de build local. O processo inteiro, com o porquê de cada regra, está em `AUTOMACAO.md`.
