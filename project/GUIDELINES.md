# Diretriz de código (IMPORTANTE — sempre seguir)

**Todo código novo deve ser escrito em inglês — sem exceção.**
- Strings visíveis ao usuário (UI, mensagens de erro, labels, placeholders): inglês
- Nomes de variáveis, funções, classes, arquivos: inglês
- Comentários no código: podem ficar em português (não são visíveis ao usuário e facilitam o aprendizado)
- Esta regra vale para todos os arquivos: `.tsx`, `.ts`, `.rs`

**I18n (P136, Sessão 268): inglês é o padrão e `pt-BR` é o segundo idioma.**
- **Todo texto que a pessoa lê passa por `t("chave")`**, nunca fica escrito no componente. O texto novo entra no catálogo em inglês (`src/i18n/en/<parte>.ts`) e no `pt-BR` (`src/i18n/pt-BR/<parte>.ts`); o `tsc` acusa uma chave que falta no `pt-BR`, e o teste `tests/i18n.test.mjs` confere que as duas línguas têm as mesmas chaves e os mesmos `{nomes}`.
- Chave por tela e por texto (`settings.providers.add`), nunca a frase como chave. Número e nome entram por `{nome}` (`t("vault.listError", { error })`); um texto com elemento dentro (um `<code>`) usa `rich(chave, { nome: <code/> })`; plural são duas chaves (`replies.one`, `replies.many`).
- Data, hora e número usam `intlTag()` (`en-US` ou `pt-BR`), nunca um idioma escrito no código. Constante de módulo que guarda texto vira função ou `get`, para acompanhar a troca de idioma.
- O idioma vem do que a pessoa escolheu (guardado no navegador, `warden.lang`), senão do navegador (qualquer `pt*` é `pt-BR`), senão inglês. O seletor fica no cabeçalho e na tela de entrada. No **desktop** (Sessão 270) é o mesmo módulo (`desktop/src/i18n/`), o seletor é a primeira seção de Configurações, e o `App` chama `useLocale()` para redesenhar tudo na troca; as mensagens de erro do Rust (`src-tauri`) ficam em inglês, sem passar o idioma ao backend.
- No **mobile** (Sessão 271) o texto vive em ARB (`mobile/lib/l10n/app_en.arb` e `app_pt.arb`) e a tela lê `AppLocalizations.of(context)`; depois de mexer num ARB, rode `flutter gen-l10n` e commite `lib/l10n/generated/`. Função de serviço sem `BuildContext` recebe o `AppLocalizations` por parâmetro (ou usa `AppLanguage.current()` numa notificação). Teste de widget monta o app com `testApp()`. Chave de widget nunca vem do rótulo traduzido. `test/l10n_test.dart` confere chaves e `{nomes}` iguais.
- O texto que o **hub** manda (`key_check.rs`, `settings.rs`) fica em inglês: o hub não sabe o idioma da página (decisão do usuário, P167 a).
- Fora do escopo desta rodada (P136b): comentários, logs e nomes de teste em português; os `project/*.md` ficam em português (são do projeto, não do produto).

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
