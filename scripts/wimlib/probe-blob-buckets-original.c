/* Test-only original internal bucket algorithm; headers/source are unchanged. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wimlib.h>
#include "wimlib/blob_table.h"
#include "wimlib/resource.h"
static int fail_growth;
static void *allocate(size_t bytes){if(fail_growth&&bytes==128*sizeof(void *)){fail_growth=0;return NULL;}return malloc(bytes);}
static int print(struct blob_descriptor *blob,void *context){(void)context;printf(" %zu",blob->hash_short/256);return 0;}
static int stop(struct blob_descriptor *blob,void *context){(void)blob;int *left=context;return --*left==0?37:0;}
static void dump(struct blob_table *table,const char *label){printf("%s",label);int rc=for_blob_in_table(table,print,NULL);printf(" rc%d cap%zu count%zu\n",rc,table->mask+1,table->num_blobs);}
int main(void){
    wimlib_set_memory_allocator(allocate,free,realloc);
    printf("word %zu disk %zu\n",sizeof(size_t),sizeof(struct blob_descriptor_disk));
    struct blob_table *table=new_blob_table(64);if(!table)return 2;
    struct blob_descriptor *blobs[130];
    for(size_t i=0;i<130;i++){
        blobs[i]=new_blob_descriptor();if(!blobs[i])return 3;
        memset(blobs[i]->hash,0,sizeof(blobs[i]->hash));blobs[i]->hash_short=i*256;
        blobs[i]->hash[19]=1;blobs[i]->size=4;blobs[i]->refcnt=1;
        blob_table_insert(table,blobs[i]);
        if(i==63)dump(table,"at64");if(i==64)dump(table,"at65");
        if(i==65)dump(table,"at66");if(i==127)dump(table,"at128");
        if(i==128)dump(table,"at129");if(i==129)dump(table,"at130");
    }
    blob_table_unlink(table,blobs[4]);blob_table_insert(table,blobs[4]);dump(table,"reinsert4");
    for(int count=1;count<=130;count++){int left=count;printf("stop %d %d\n",count,for_blob_in_table(table,stop,&left));}
    free_blob_table(table);
    table=new_blob_table(64);if(!table)return 4;
    for(size_t i=0;i<66;i++){
        blobs[i]=new_blob_descriptor();if(!blobs[i])return 5;
        memset(blobs[i]->hash,0,sizeof(blobs[i]->hash));blobs[i]->hash_short=i*256;
        blobs[i]->hash[19]=1;blobs[i]->size=4;blobs[i]->refcnt=1;
        if(i==64)fail_growth=1;
        blob_table_insert(table,blobs[i]);
        if(i==64)dump(table,"failed65");if(i==65)dump(table,"recovered66");
    }
    free_blob_table(table);return 0;
}
