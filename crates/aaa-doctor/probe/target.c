/* aaa doctor probe의 첫 실행 측정 대상. 실행할 때마다 salt를 바꿔 「처음 보는 실행 파일」을 만든다. */
volatile const char aa_probe_salt[] = "AAA_PROBE_SALT_MARKER_0123456789AB";
int main(void) { return aa_probe_salt[0] == 0; }
