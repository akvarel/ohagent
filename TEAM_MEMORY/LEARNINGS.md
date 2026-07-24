# Learnings — Avion non-native build & deploy

## Build-order invariant (critical)

**Problem:** После коммита pom.xml хеш меняется. Если строить образ до коммита deployment.yml — тег в `deployment.yml` уже не совпадает с образом в registry, потому что deployment.yml даёт новый хеш.

**Правильный порядок:**

```
1. Закоммитить ВСЕ изменения в submodule (pom.xml, deployment.yml — всё)
2. Получить финальный git hash:  git rev-parse --short HEAD
3. Этот hash = тег образа
4. docker build -t registry/service:$hash
5. docker push
6. kubectl set image deployment/service service=registry/service:$hash
```

**Неправильный порядок (что сделали):**

```
❌ Закоммитили pom.xml → hash стал A
❌ Собрали образ с тегом A ✅ образ существует
❌ Закоммитили deployment.yml → hash стал B
❌ kubectl set image → deploy ищет образ с тегом B ❌ не найден
```

**Проверка перед деплоем:**

```bash
hash=$(git rev-parse --short HEAD)
tag=$(grep "image:" deployment/deployment.yml | sed 's/.*://')
if [ "$hash" != "$tag" ]; then
  echo "❌ MISMATCH — deploy tag != commit hash"
fi
```

## Docker secrets

Два паттерна для Maven credentials в Dockerfile:

1. **Современный (секреты):** `--mount=type=secret,id=repo_username` + `--secret id=repo_username,env=REPO_USERNAME`
2. **Старый (build-arg):** `ARG REPO_USERNAME` + `--build-arg REPO_USERNAME=$REPO_USERNAME`

У `avion-ratehawk` Dockerfile использует `ARG`/`ENV`, а не `--mount=type=secret`. Строить с `--build-arg`, а не с `--secret`.

## Non-native build

- Dockerfile: `Dockerfile.nonNative`
- JDK base: `ghcr.io/graalvm/jdk-community:25`
- JRE base: `ghcr.io/bell-sw/liberica-runtime-container:jre-25-musl`
- Maven: `-DskipTests=true -Djacoco.skip=true`
- Результат: JAR ~50-80 MB, per-service build ~45-90s
- Registry: `rg.pl-waw.scw.cloud/avion/<service>:<hash>`
- Parallel batches of 4 (ограничение CPU + RAM на локальной машине)

## K8s Deployment

- Namespace: `avion`
- Deployment name: `<service>-deployment` (кроме `avion-telegram` → `telegram-bot-deployment`)
- После `kubectl set image` нужно ждать rollout: `kubectl rollout status deployment -n avion --timeout=300s`
