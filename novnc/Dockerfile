FROM alpine:latest

RUN apk update && apk add git bash python3 && mkdir /app

WORKDIR /app

RUN git clone https://github.com/novnc/noVNC.git

COPY start.sh /app/start.sh

ENV VNC_HOST mknm-os:5900
ENV LISTEN_PORT 5090

CMD /app/start.sh $VNC_HOST $LISTEN_PORT
