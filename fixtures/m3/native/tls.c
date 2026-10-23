static __thread int value = 41;
int tls_next(int delta) {
  value += delta;
  return value;
}
