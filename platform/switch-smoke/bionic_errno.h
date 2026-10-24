/* Linux/Bionic errno at the guest boundary; host errors retain newlib values.
 * Numeric ABI constants are from Android NDK asm-generic errno headers.
 * Cygwin/MSYS also uses non-Linux host errno numbers and needs the same
 * symbolic conversion. The Horizon branch remains unchanged.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#pragma once
#include <errno.h>
static inline int as_errno_to_android(int value) {
#if defined(__SWITCH__) || defined(__CYGWIN__)
  switch (value) {
#ifdef EDEADLK
  case EDEADLK:
    return 35;
#endif
#ifdef ENAMETOOLONG
  case ENAMETOOLONG:
    return 36;
#endif
#ifdef ENOLCK
  case ENOLCK:
    return 37;
#endif
#ifdef ENOSYS
  case ENOSYS:
    return 38;
#endif
#ifdef ENOTEMPTY
  case ENOTEMPTY:
    return 39;
#endif
#ifdef ELOOP
  case ELOOP:
    return 40;
#endif
#ifdef ENOMSG
  case ENOMSG:
    return 42;
#endif
#ifdef EIDRM
  case EIDRM:
    return 43;
#endif
#ifdef ECHRNG
  case ECHRNG:
    return 44;
#endif
#ifdef EL2NSYNC
  case EL2NSYNC:
    return 45;
#endif
#ifdef EL3HLT
  case EL3HLT:
    return 46;
#endif
#ifdef EL3RST
  case EL3RST:
    return 47;
#endif
#ifdef ELNRNG
  case ELNRNG:
    return 48;
#endif
#ifdef EUNATCH
  case EUNATCH:
    return 49;
#endif
#ifdef ENOCSI
  case ENOCSI:
    return 50;
#endif
#ifdef EL2HLT
  case EL2HLT:
    return 51;
#endif
#ifdef EBADE
  case EBADE:
    return 52;
#endif
#ifdef EBADR
  case EBADR:
    return 53;
#endif
#ifdef EXFULL
  case EXFULL:
    return 54;
#endif
#ifdef ENOANO
  case ENOANO:
    return 55;
#endif
#ifdef EBADRQC
  case EBADRQC:
    return 56;
#endif
#ifdef EBADSLT
  case EBADSLT:
    return 57;
#endif
#ifdef EBFONT
  case EBFONT:
    return 59;
#endif
#ifdef EMULTIHOP
  case EMULTIHOP:
    return 72;
#endif
#ifdef EDOTDOT
  case EDOTDOT:
    return 73;
#endif
#ifdef EBADMSG
  case EBADMSG:
    return 74;
#endif
#ifdef EOVERFLOW
  case EOVERFLOW:
    return 75;
#endif
#ifdef ENOTUNIQ
  case ENOTUNIQ:
    return 76;
#endif
#ifdef EBADFD
  case EBADFD:
    return 77;
#endif
#ifdef EREMCHG
  case EREMCHG:
    return 78;
#endif
#ifdef ELIBACC
  case ELIBACC:
    return 79;
#endif
#ifdef ELIBBAD
  case ELIBBAD:
    return 80;
#endif
#ifdef ELIBSCN
  case ELIBSCN:
    return 81;
#endif
#ifdef ELIBMAX
  case ELIBMAX:
    return 82;
#endif
#ifdef ELIBEXEC
  case ELIBEXEC:
    return 83;
#endif
#ifdef EILSEQ
  case EILSEQ:
    return 84;
#endif
#ifdef ESTRPIPE
  case ESTRPIPE:
    return 86;
#endif
#ifdef EUSERS
  case EUSERS:
    return 87;
#endif
#ifdef ENOTSOCK
  case ENOTSOCK:
    return 88;
#endif
#ifdef EDESTADDRREQ
  case EDESTADDRREQ:
    return 89;
#endif
#ifdef EMSGSIZE
  case EMSGSIZE:
    return 90;
#endif
#ifdef EPROTOTYPE
  case EPROTOTYPE:
    return 91;
#endif
#ifdef ENOPROTOOPT
  case ENOPROTOOPT:
    return 92;
#endif
#ifdef EPROTONOSUPPORT
  case EPROTONOSUPPORT:
    return 93;
#endif
#ifdef ESOCKTNOSUPPORT
  case ESOCKTNOSUPPORT:
    return 94;
#endif
#ifdef EAFNOSUPPORT
  case EAFNOSUPPORT:
    return 97;
#endif
#ifdef EADDRINUSE
  case EADDRINUSE:
    return 98;
#endif
#ifdef EADDRNOTAVAIL
  case EADDRNOTAVAIL:
    return 99;
#endif
#ifdef ENETDOWN
  case ENETDOWN:
    return 100;
#endif
#ifdef ENETUNREACH
  case ENETUNREACH:
    return 101;
#endif
#ifdef ENETRESET
  case ENETRESET:
    return 102;
#endif
#ifdef ECONNABORTED
  case ECONNABORTED:
    return 103;
#endif
#ifdef EISCONN
  case EISCONN:
    return 106;
#endif
#ifdef ENOTCONN
  case ENOTCONN:
    return 107;
#endif
#ifdef ESHUTDOWN
  case ESHUTDOWN:
    return 108;
#endif
#ifdef ETOOMANYREFS
  case ETOOMANYREFS:
    return 109;
#endif
#ifdef ETIMEDOUT
  case ETIMEDOUT:
    return 110;
#endif
#ifdef EHOSTDOWN
  case EHOSTDOWN:
    return 112;
#endif
#ifdef EHOSTUNREACH
  case EHOSTUNREACH:
    return 113;
#endif
#ifdef EALREADY
  case EALREADY:
    return 114;
#endif
#ifdef EINPROGRESS
  case EINPROGRESS:
    return 115;
#endif
#ifdef ESTALE
  case ESTALE:
    return 116;
#endif
#ifdef EDQUOT
  case EDQUOT:
    return 122;
#endif
#ifdef ENOMEDIUM
  case ENOMEDIUM:
    return 123;
#endif
#ifdef ECANCELED
  case ECANCELED:
    return 125;
#endif
#ifdef EOWNERDEAD
  case EOWNERDEAD:
    return 130;
#endif
#ifdef ENOTRECOVERABLE
  case ENOTRECOVERABLE:
    return 131;
#endif
#ifdef ENOTSUP
  case ENOTSUP:
    return 95;
#endif
  default:
    return value;
  }
#else
  return value;
#endif
}
static inline int as_errno_from_android(int value) {
#if defined(__SWITCH__) || defined(__CYGWIN__)
  switch (value) {
#ifdef EDEADLK
  case 35:
    return EDEADLK;
#endif
#ifdef ENAMETOOLONG
  case 36:
    return ENAMETOOLONG;
#endif
#ifdef ENOLCK
  case 37:
    return ENOLCK;
#endif
#ifdef ENOSYS
  case 38:
    return ENOSYS;
#endif
#ifdef ENOTEMPTY
  case 39:
    return ENOTEMPTY;
#endif
#ifdef ELOOP
  case 40:
    return ELOOP;
#endif
#ifdef ENOMSG
  case 42:
    return ENOMSG;
#endif
#ifdef EIDRM
  case 43:
    return EIDRM;
#endif
#ifdef ECHRNG
  case 44:
    return ECHRNG;
#endif
#ifdef EL2NSYNC
  case 45:
    return EL2NSYNC;
#endif
#ifdef EL3HLT
  case 46:
    return EL3HLT;
#endif
#ifdef EL3RST
  case 47:
    return EL3RST;
#endif
#ifdef ELNRNG
  case 48:
    return ELNRNG;
#endif
#ifdef EUNATCH
  case 49:
    return EUNATCH;
#endif
#ifdef ENOCSI
  case 50:
    return ENOCSI;
#endif
#ifdef EL2HLT
  case 51:
    return EL2HLT;
#endif
#ifdef EBADE
  case 52:
    return EBADE;
#endif
#ifdef EBADR
  case 53:
    return EBADR;
#endif
#ifdef EXFULL
  case 54:
    return EXFULL;
#endif
#ifdef ENOANO
  case 55:
    return ENOANO;
#endif
#ifdef EBADRQC
  case 56:
    return EBADRQC;
#endif
#ifdef EBADSLT
  case 57:
    return EBADSLT;
#endif
#ifdef EBFONT
  case 59:
    return EBFONT;
#endif
#ifdef EMULTIHOP
  case 72:
    return EMULTIHOP;
#endif
#ifdef EDOTDOT
  case 73:
    return EDOTDOT;
#endif
#ifdef EBADMSG
  case 74:
    return EBADMSG;
#endif
#ifdef EOVERFLOW
  case 75:
    return EOVERFLOW;
#endif
#ifdef ENOTUNIQ
  case 76:
    return ENOTUNIQ;
#endif
#ifdef EBADFD
  case 77:
    return EBADFD;
#endif
#ifdef EREMCHG
  case 78:
    return EREMCHG;
#endif
#ifdef ELIBACC
  case 79:
    return ELIBACC;
#endif
#ifdef ELIBBAD
  case 80:
    return ELIBBAD;
#endif
#ifdef ELIBSCN
  case 81:
    return ELIBSCN;
#endif
#ifdef ELIBMAX
  case 82:
    return ELIBMAX;
#endif
#ifdef ELIBEXEC
  case 83:
    return ELIBEXEC;
#endif
#ifdef EILSEQ
  case 84:
    return EILSEQ;
#endif
#ifdef ESTRPIPE
  case 86:
    return ESTRPIPE;
#endif
#ifdef EUSERS
  case 87:
    return EUSERS;
#endif
#ifdef ENOTSOCK
  case 88:
    return ENOTSOCK;
#endif
#ifdef EDESTADDRREQ
  case 89:
    return EDESTADDRREQ;
#endif
#ifdef EMSGSIZE
  case 90:
    return EMSGSIZE;
#endif
#ifdef EPROTOTYPE
  case 91:
    return EPROTOTYPE;
#endif
#ifdef ENOPROTOOPT
  case 92:
    return ENOPROTOOPT;
#endif
#ifdef EPROTONOSUPPORT
  case 93:
    return EPROTONOSUPPORT;
#endif
#ifdef ESOCKTNOSUPPORT
  case 94:
    return ESOCKTNOSUPPORT;
#endif
#ifdef EAFNOSUPPORT
  case 97:
    return EAFNOSUPPORT;
#endif
#ifdef EADDRINUSE
  case 98:
    return EADDRINUSE;
#endif
#ifdef EADDRNOTAVAIL
  case 99:
    return EADDRNOTAVAIL;
#endif
#ifdef ENETDOWN
  case 100:
    return ENETDOWN;
#endif
#ifdef ENETUNREACH
  case 101:
    return ENETUNREACH;
#endif
#ifdef ENETRESET
  case 102:
    return ENETRESET;
#endif
#ifdef ECONNABORTED
  case 103:
    return ECONNABORTED;
#endif
#ifdef EISCONN
  case 106:
    return EISCONN;
#endif
#ifdef ENOTCONN
  case 107:
    return ENOTCONN;
#endif
#ifdef ESHUTDOWN
  case 108:
    return ESHUTDOWN;
#endif
#ifdef ETOOMANYREFS
  case 109:
    return ETOOMANYREFS;
#endif
#ifdef ETIMEDOUT
  case 110:
    return ETIMEDOUT;
#endif
#ifdef EHOSTDOWN
  case 112:
    return EHOSTDOWN;
#endif
#ifdef EHOSTUNREACH
  case 113:
    return EHOSTUNREACH;
#endif
#ifdef EALREADY
  case 114:
    return EALREADY;
#endif
#ifdef EINPROGRESS
  case 115:
    return EINPROGRESS;
#endif
#ifdef ESTALE
  case 116:
    return ESTALE;
#endif
#ifdef EDQUOT
  case 122:
    return EDQUOT;
#endif
#ifdef ENOMEDIUM
  case 123:
    return ENOMEDIUM;
#endif
#ifdef ECANCELED
  case 125:
    return ECANCELED;
#endif
#ifdef EOWNERDEAD
  case 130:
    return EOWNERDEAD;
#endif
#ifdef ENOTRECOVERABLE
  case 131:
    return ENOTRECOVERABLE;
#endif
#ifdef ENOTSUP
  case 95:
    return ENOTSUP;
#endif
  default:
    return value;
  }
#else
  return value;
#endif
}
