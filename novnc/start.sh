if [ $# -ne 2 ]; then 
    echo "usage: $0 <vnc-host:vnc-port> <listen-port>"
    exit 1
fi

cd noVNC
./utils/novnc_proxy --record tmp --vnc $1 --listen $2
