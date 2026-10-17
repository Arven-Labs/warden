# Guia de uso do Warden

Este guia leva do zero até uma conversa com o seu agente. Não precisa compilar nada: tudo vem do
[release](https://github.com/Arven-Labs/warden/releases/latest).

## 1. Escolha o caminho

| | **A. Só o app no computador** | **B. Hub sempre ligado** |
|---|---|---|
| Para quem | testar e usar num computador só | usar pelo celular e pelo navegador, ter tarefas agendadas rodando com o PC desligado |
| O que instala | o app (desktop) | a imagem Docker `warden-server` numa VPS, num servidor de casa ou no próprio PC |
| Precisa de | uma chave de modelo (ou o Ollama local) | Docker, uma chave de modelo e uma porta (7420) |

Dá para começar pela A e acrescentar a B depois: o app se conecta a qualquer hub.

## 2. Caminho A: o app no computador

### Instalar

Baixe o arquivo do seu sistema na página do release:

| Sistema | Arquivo | Como |
|---|---|---|
| Debian, Ubuntu | `Warden_<versão>_amd64.deb` | `sudo apt install ./Warden_*_amd64.deb` (o `apt` puxa as bibliotecas que faltarem) |
| Fedora | `Warden-<versão>-1.x86_64.rpm` | `sudo dnf install ./Warden-*.rpm` |
| Qualquer Linux | `Warden_<versão>_amd64.AppImage` | `chmod +x Warden_*.AppImage && ./Warden_*.AppImage` |
| Windows | `Warden_<versão>_x64_en-US.msi` (ou `..._x64-setup.exe`) | dois cliques |
| macOS (Apple Silicon) | `Warden_<versão>_aarch64.dmg` | arraste para Aplicativos |

Os instaladores ainda não são assinados: o Windows (SmartScreen) e o macOS (Gatekeeper) avisam que o
autor é desconhecido. É esperado; confirme que o arquivo veio do release acima.

### Ligar um modelo

1. Abra o Warden e vá em **Settings → Model providers → + Add provider**.
2. Escolha o tipo e preencha:
   - **gemini**, **openai**, **anthropic**: cole a chave da conta.
   - **openai_compatible**: qualquer servidor no formato da OpenAI (Ollama, OpenRouter, Groq, DeepSeek). Informe o
     `base_url` (o do Ollama é `http://localhost:11434/v1`; a chave pode ser qualquer texto) e o nome do modelo.
3. Marque o provedor como ativo e salve.
4. Abra uma conversa e escreva algo.

As chaves ficam em `config.secrets.toml` (só o seu usuário lê), e não no `config.toml`, que é o arquivo
que se copia e se cola por aí.

### Onde ficam as coisas

| O quê | Onde (Linux) |
|---|---|
| Configuração | `~/.config/warden/config.toml` |
| Chaves dos provedores | `~/.config/warden/config.secrets.toml` |
| Cofre de memória (notas em markdown) | `~/Warden/vault` (muda em Settings) |

No Windows e no macOS a configuração fica na pasta de configuração do sistema, em `warden`. O cofre é uma pasta de
arquivos `.md` que o Obsidian também abre.

### Deixar o computador ser o hub (opcional)

Para o celular ou outro computador usar este app como servidor, vá em **Workspace → Ser o hub desta rede**, escolha a
porta e ligue. Depois de ligado, ele volta sozinho toda vez que o app abre. Os outros aparelhos da mesma rede o
encontram sozinhos. Para usar de fora de casa, o caminho B é melhor: ele não depende do PC estar ligado.

## 3. Caminho B: um hub sempre ligado (Docker)

Serve uma VPS pequena. O hub ocioso usa cerca de 25 MB de memória nesta imagem.

### 3.1 Subir

```bash
# a chave de pareamento: quem a tem é o dono. Guarde-a (gerenciador de senhas).
KEY="$(openssl rand -hex 32)"
echo "$KEY"

docker volume create warden-data
```

Crie a configuração dentro do volume. Troque `SUA_CHAVE_DE_MODELO` e, se não for o Gemini, o tipo e o modelo:

```toml
active_provider = "principal"

[[providers]]
id = "principal"
kind = "gemini"
api_key = "SUA_CHAVE_DE_MODELO"
model = "gemini-3.5-flash"
```

Salve o texto acima como `config.toml` e copie para o volume, rodando com o mesmo usuário da imagem:

```bash
docker run --rm -i --entrypoint sh -v warden-data:/data ghcr.io/arven-labs/warden-server:latest \
  -c 'cat > /data/config.toml' < config.toml
```

Suba o hub:

```bash
docker run -d --name warden --restart unless-stopped \
  --read-only --tmpfs /tmp -v warden-data:/data -p 7420:7420 \
  -e WARDEN_SERVER_AUTH_KEY="$KEY" \
  ghcr.io/arven-labs/warden-server:latest serve --no-embedding-warm-up --run-tasks
```

`--run-tasks` liga as tarefas agendadas neste hub; deixe só em um hub. Confira:

```bash
curl http://localhost:7420/health        # {"status":"ok", ...}
```

Os comandos de administração rodam dentro do contêiner:

```bash
warden() { docker exec warden warden-server --data-dir /data "$@"; }
warden api-keys create meu-script     # chave da API compatível com a OpenAI (aparece uma vez)
warden tasks list
```

### 3.2 Conectar os aparelhos

- **Navegador**: abra `http://<endereço do hub>:7420`. A primeira entrada pede a chave de pareamento (`$KEY`).
- **App desktop**: em **Workspace → Hubs → Add hub** informe o endereço (`ws://<endereço>:7420`) e a chave. Na mesma
  rede, o botão de descoberta acha o hub sozinho.
- **Celular**: o app Flutter se conecta do mesmo jeito. O release ainda não traz o APK; hoje ele é construído com
  `cd mobile && flutter build apk`.

### 3.3 Fora de casa: use TLS

Sem criptografia, a chave de pareamento e as conversas viajam em claro. Para sair da rede local, escolha um:

- **Tailscale** (o mais simples): instale o Tailscale na máquina do hub e nos seus aparelhos, e suba o hub com
  `--tailscale-cert` (precisa ligar os certificados HTTPS no painel do Tailscale). O endereço passa a ser
  `https://<máquina>.<tailnet>.ts.net:7420`.
- **Seu próprio certificado**: `--tls-cert fullchain.pem --tls-key privkey.pem --tls-host seu.dominio`.

Não publique a porta 7420 na internet sem TLS.

### 3.4 Sem Docker (systemd)

Se preferir rodar o binário `warden-server` direto (compilado com `cargo build --release -p warden-server`):

```ini
# /etc/systemd/system/warden.service
[Service]
User=warden
Environment=WARDEN_SERVER_AUTH_KEY=<a chave>
ExecStart=/usr/local/bin/warden-server --data-dir /var/lib/warden serve --no-embedding-warm-up --run-tasks
Restart=on-failure
StateDirectory=warden

[Install]
WantedBy=multi-user.target
```

## 4. No dia a dia

O README tem cada recurso com os comandos. Os mais úteis para começar:

- **Tarefas agendadas** ("todo dia útil às 8, resuma as notícias"): `warden tasks add`, ou a aba Tarefas / Tasks. Só
  rodam em um hub com `--run-tasks`.
- **Agentes** com personalidade, memória e ferramentas próprias: em Settings, ou pelo terminal (`warden`, comando `/agents`).
- **Skills**: instruções que o modelo carrega quando precisa; peça uma na conversa ("crie uma skill para...").
- **API compatível com a OpenAI**: `http://<hub>:7420/v1`, com uma chave de `warden api-keys create`.
- **Mais pessoas** (família, equipe): `warden users add ana`, cada uma com cofre e conversas cifrados.
- **Emprestar outra máquina** (um PC de casa, uma segunda VPS) aos agentes: `warden-server node ...`.

## 5. Backup e atualização

O que importa guardar: o **volume `/data`** (ou a pasta de configuração e o cofre, no caminho A) e a **chave de
pareamento**.

```bash
# uma cópia do volume inteiro (config, chaves dos provedores, cofre, conversas, dispositivos)
docker stop warden
docker run --rm -v warden-data:/data:ro -v "$PWD":/out debian:trixie-slim \
  tar czf /out/warden-data-$(date +%F).tgz -C /data .
docker start warden
```

Esse arquivo **não é cifrado e contém as chaves dos seus provedores**: guarde-o como guarda uma senha. Para restaurar,
crie um volume novo e extraia o `.tgz` nele (`tar xzf ... -C /data`, com o mesmo usuário da imagem, `--user 10001`).
O comando `warden backup --out <pasta vazia>` é outra coisa: copia só os dados **dos membros** (seção 4), já cifrados
com a senha de cada um.

Para atualizar o hub:

```bash
docker pull ghcr.io/arven-labs/warden-server:latest
docker rm -f warden            # os dados ficam no volume
# e rode de novo o "docker run -d ..." da seção 3.1
```

No caminho A, baixe a nova versão no release e instale por cima; a configuração e o cofre ficam.

## 6. Se algo falhar

| Sintoma | O que olhar |
|---|---|
| O hub não responde | `docker logs warden`; `curl localhost:7420/health`; a porta 7420 já está em uso? |
| O contêiner morre ao subir | falta `WARDEN_SERVER_AUTH_KEY` (mínimo de 32 caracteres) ou o `config.toml` tem um campo que não existe |
| "no API key configured" | a chave do provedor ativo não foi preenchida em Settings, ou está vazia no `config.toml` |
| O navegador não pede/aceita a chave | confira se é a chave do `WARDEN_SERVER_AUTH_KEY` e se o endereço é `http://` (sem TLS) ou `https://` |
| O app não abre no Linux | instale pelo `.deb`/`.rpm` (puxam as bibliotecas) e rode no terminal para ver a mensagem de erro (o executável do `.deb` se chama `desktop`; o AppImage, o próprio arquivo) |
| Respostas lentas na primeira busca | o modelo de busca por significado baixa na primeira vez e fica em `/data/models` |

## 7. O que ainda não foi conferido com um modelo de verdade

Honestidade sobre o estágio: o projeto tem muitos testes automáticos, mas vários recursos só rodaram contra um modelo de
mentira. Se algo estranho aparecer, é provável que esteja aqui: **skills, tarefas agendadas, delegação entre agentes,
anexo de imagem, voz e leitura em voz alta**, nós em máquinas diferentes e o **app no iPhone**. Cada um está listado em
`project/PENDING.md` (P29, P30, P31, P60, P73, P95, P96, P129). Se você achar um defeito, anote o que fez e o que
esperava.
