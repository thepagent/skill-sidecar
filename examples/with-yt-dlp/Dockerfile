FROM ghcr.io/thepagent/skill-sidecar:main
RUN apt-get update && apt-get install -y python3-pip && \
    pip3 install --break-system-packages yt-dlp && \
    rm -rf /var/lib/apt/lists/*
