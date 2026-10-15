# SPEC: Warden, router de modelos e expectativa de futuro

> Destinatário: agente que trabalha no repositório do Warden (agente de IA pessoal).
> Esta spec descreve **intenção e princípios**, não implementação.

## Regra número um para você (agente)

**Não presuma nada.** Nem estrutura de arquivos, nem código, nem convenções, nem o que existe no repositório. A conversa que originou esta spec foi de alto nível e a realidade do código pode estar diferente do que foi imaginado. Antes de propor qualquer coisa:

1. Explore o repositório e entenda o que realmente existe (incluindo o roteador de APIs de LLM atual, o "Nine Router").
2. Apresente ao Fabio o que encontrou.
3. Discuta as dúvidas e **confirme cada decisão de arquitetura com ele** ("vai ser assim mesmo?").
4. Quando houver mais de um caminho razoável, apresente opções com prós e contras e deixe-o escolher.

Itens marcados como **[DIREÇÃO]** são ideias a validar, não decisões.

## Contexto

Fabio cogitou migrar o roteador de modelos do Warden para o Lume, transformando o Lume num "centro de APIs". Depois de discutir, a direção mudou: **o Lume será desfeito** (ver `SPEC-lume-desmontagem.md`), as capacidades financeiras ficam no Anchor, e o roteamento de modelos de IA **fica no Warden**. Princípio: um aplicativo central por área (IA no Warden, finanças no Anchor), poucos repositórios, máxima organização.

Nenhum projeto dele tem usuários ainda; ele constrói primeiro para uso próprio e quer acertar a arquitetura agora para não sofrer na hora de escalar.

## Decisão tomada

- O roteador de modelos permanece no Warden e **não migra** para o Lume.
- O Warden deve funcionar como **cliente das capacidades financeiras do Anchor** (a intenção anterior era integração via MCP; confirme).

## Visão de futuro (expectativa a respeitar desde já)

1. **O usuário escolhe o modelo** que quer usar.
2. **Três formas de ter acesso a modelos**, e o usuário não deve ser obrigado a usar nenhuma específica:
   - Colocar a própria chave de API (BYO).
   - Cadastrar os próprios modelos e as próprias APIs (endpoints customizados). Fabio acha isso justo e desejável.
   - Usar a **nuvem do Fabio**, onde ele paga o modelo pelo usuário: via plano ou via tokens, num estilo OpenRouter. O usuário só escolhe o modelo e usa.
3. **Combinação de várias APIs em um endpoint único**: o usuário cadastra 4 ou 5 APIs/modelos e o sistema expõe uma só, com a lógica de roteamento por trás.
4. **Mesmo código para self-host e hospedado.** Quem quiser hospeda o seu; quem quiser paga pelo hospedado do Fabio. A ideia de longo prazo inclui um "OpenRouter próprio".
5. Princípio do ecossistema: o grátis/self-hosted sempre funciona com qualidade padrão; o pago é conveniência ou qualidade extra, nunca funcionalidade básica trancada.

## [DIREÇÃO] Ideias discutidas (validar, não presumir)

- Tratar a nuvem do Fabio como **mais um provider** dentro do mesmo mecanismo de resolução de acesso do router, em vez de um produto separado.
- Ordem de resolução possível: chave própria do usuário, depois endpoint customizado, depois nuvem do Fabio.
- Deixar **ganchos de medição de uso (metering)** desde o início, mesmo que inicialmente não façam nada, para evitar retrabalho quando a versão hospedada existir.
- Manter chaves de API de terceiros isoladas de outros dados, por segurança.
- Se o router hospedado virar um produto de fato (multi-tenant, exposto na internet, com abuso e custódia de chaves), pode merecer um repositório próprio. Por ora, o objetivo é uma fronteira de módulo limpa dentro do Warden que permita essa extração.

## Questões em aberto (discuta com Fabio)

- **Cobrança e planos:** se o usuário tiver plano do Warden e usar a nuvem de modelos, como funciona o abatimento entre planos? Fabio ainda não decidiu e quer avaliar as questões éticas. A ideia discutida foi centralizar conta, planos e pagamentos no site mãe da organização, com Warden e Anchor apenas validando permissões. Confirme se continua valendo.
- Como o modelo de custo da nuvem de modelos funciona (plano, tokens, créditos pré-pagos) e como limitar abuso.
- Qual a melhor forma de expor o router (endpoint único compatível com algum padrão existente?).
- Como a integração com o Anchor será feita na prática.
- O que, do estado atual do Nine Router, deve ser mantido, refatorado ou reescrito.

## Preferências de Fabio que devem guiar as decisões

- Mínimo de repositórios possível; centralizar por organização, descentralizar só quando necessário.
- Evitar "pontas soltas".
- Arquitetura pensada para o futuro, sem retrabalho para escalar.
- Tradeoffs apresentados antes de implementar.

## Fora de escopo

- Mudanças no TruthID. O Warden não deve integrar com ele além do que já foi decidido (login opcional via plugin).
- Criar novos repositórios.
