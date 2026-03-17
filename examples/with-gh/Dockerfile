FROM ghcr.io/thepagent/skill-sidecar:main
RUN apt-get update && apt-get install -y curl && \
    curl -fsSL https://github.com/cli/cli/releases/latest/download/gh_$(curl -s https://api.github.com/repos/cli/cli/releases/latest | grep tag_name | cut -d'"' -f4 | tr -d v)_linux_amd64.tar.gz \
    | tar -xz --strip-components=2 -C /usr/local/bin '*/bin/gh' && \
    rm -rf /var/lib/apt/lists/*
