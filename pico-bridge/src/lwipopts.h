#pragma once

// lwIP configuration tuned for a single-port UDP application on Pico W.
// Keeps the heap and pbuf counts low: we never use TCP, DNS, DHCPv6, or
// anything else, just UDP.

#define NO_SYS                      1
#define LWIP_SOCKET                 0
#define LWIP_NETCONN                0
#define LWIP_RAW                    1
#define LWIP_UDP                    1
#define LWIP_TCP                    0
#define LWIP_DHCP                   1
#define LWIP_AUTOIP                 1
#define LWIP_ICMP                   1
#define LWIP_DNS                    0
#define LWIP_IGMP                   0
#define LWIP_ARP                    1
#define LWIP_NETIF_LOOPBACK         0
#define LWIP_NETCONN_FULLDUPLEX     0

#define LWIP_IPV6                   0
#define LWIP_IPV4                   1

#define MEM_LIBC_MALLOC             0
#define MEM_ALIGNMENT               4
#define MEM_SIZE                    8000
#define MEMP_NUM_PBUF               12
#define MEMP_NUM_UDP_PCB            6
#define MEMP_NUM_TCP_PCB            0
#define MEMP_NUM_TCP_PCB_LISTEN     0
#define MEMP_NUM_TCP_SEG            0
#define MEMP_NUM_REASSDATA          1
#define MEMP_NUM_ARP_QUEUE          5
#define PBUF_POOL_SIZE              16

#define LWIP_NETIF_HOSTNAME         1
#define LWIP_CHECKSUM_ON_COPY       1

#define TCPIP_THREAD_STACKSIZE      1024
#define DEFAULT_RAW_RECVMBOX_SIZE   8
#define DEFAULT_UDP_RECVMBOX_SIZE   8
#define DEFAULT_TCP_RECVMBOX_SIZE   0
#define DEFAULT_ACCEPTMBOX_SIZE     0
#define TCPIP_MBOX_SIZE             8

// Stats / debug off in release; the diag_log ring buffer is what we use
// for support output.
#define LWIP_STATS                  0
#define LWIP_DEBUG                  0
