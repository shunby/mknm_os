FROM rustlang/rust:nightly

ARG USER_NAME=nekochan
ARG USER_UID=1000
ARG USER_GID=1000

RUN apt-get update && apt-get install -y \
    llvm-dev \
    lld \
    clang \
    libc6-dev-i386 \
    qemu-system-x86 \
    dosfstools \
    mtools \
    sudo \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --gid $USER_GID $USER_NAME \
    && useradd -s /bin/bash --uid $USER_UID --gid $USER_GID -m $USER_NAME \
    && apt-get install -y sudo \
    && echo $USER_NAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USER_NAME \
    && chmod 0440 /etc/sudoers.d/$USER_NAME \
    && mkdir /app && chown $USER_NAME:$USER_NAME /app

USER $USER_NAME
WORKDIR /app

RUN rustup component add rust-src --toolchain nightly-x86_64-unknown-linux-gnu && rustup update
RUN curl -L https://github.com/uchan-nos/mikanos-build/releases/download/v2.0/x86_64-elf.tar.gz \
  | tar xzvf - -C . \
  && git clone https://github.com/uchan-nos/mikanos-build.git \
  && git clone https://github.com/novnc/noVNC.git

CMD /bin/sh -c "while sleep 1000; do :; done"
