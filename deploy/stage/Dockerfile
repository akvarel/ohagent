FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home ohagent
COPY --chmod=755 ohagent-daemon /usr/local/bin/ohagent-daemon
COPY site /opt/ohagent/site
USER 10001:10001
ENV OHAGENT_SITE_BIND=0.0.0.0 OHAGENT_DASHBOARD_DIR=/opt/ohagent/site OHAGENT_AAAS_ENABLED=0 HOME=/tmp
EXPOSE 9090
ENTRYPOINT ["ohagent-daemon", "--site-only", "--health-port", "9090"]
