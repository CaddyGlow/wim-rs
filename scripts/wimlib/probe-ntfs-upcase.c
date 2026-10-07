/* Test-only oracle linked to the unchanged original static library. */
#include <stdint.h>
#include <stdio.h>
extern uint16_t upcase[65536];
extern void init_upcase(void);
int main(void) { init_upcase(); for(unsigned i=0;i<65536;i++) { unsigned c=upcase[i]; putchar(c&255);putchar(c>>8); } return ferror(stdout)!=0; }
