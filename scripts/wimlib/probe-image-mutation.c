#include <wimlib.h>
#include <stdio.h>
#include <string.h>
static int resource(const struct wimlib_resource_entry *e, void *p){
 (void)p; printf("resource %llu %u %u %d\n",(unsigned long long)e->uncompressed_size,e->reference_count,e->part_number,e->is_metadata); return 0;
}
static void state(WIMStruct *w){
 struct wimlib_wim_info i; wimlib_get_wim_info(w,&i);
 printf("state %u %u %u\n",i.image_count,i.boot_index,i.is_marked_readonly);
 for(unsigned j=1;j<=i.image_count;j++) {
  const char *name=wimlib_get_image_name(w,j);
  printf("name %u ",j); if(name)for(const unsigned char *p=(const unsigned char *)name;*p;p++)printf("%02x",*p);else printf("NULL");puts("");
  const char *props[]={"DIRCOUNT","FILECOUNT","TOTALBYTES","HARDLINKBYTES"};
  for(unsigned k=0;k<4;k++){const char*v=wimlib_get_image_property(w,j,props[k]);printf("prop %u %s %s\n",j,props[k],v?v:"NULL");}
 }
 printf("lookup-result %d\n",wimlib_iterate_lookup_table(w,0,resource,NULL));
}
static void add(WIMStruct*w,const char*n){int index=999;int r=wimlib_add_empty_image(w,n,&index);printf("add %d %d\n",r,index);state(w);}
static void del(WIMStruct*w,int i){printf("delete %d %d\n",i,wimlib_delete_image(w,i));state(w);}
int main(int argc,char**argv){
 WIMStruct*w=NULL;int r=argc>1?wimlib_open_wim(argv[1],0,&w):wimlib_create_new_wim(0,&w);printf("start %d\n",r);if(r)return 0;
 state(w); add(w,NULL);add(w,"");add(w,"Added & <image>");add(w,"Added & <image>");add(w,"illegal\1");
 const char bad[]={ 'r','a','w',(char)0xff,0 };add(w,bad);
 del(w,0);del(w,-2);del(w,999);
 struct wimlib_wim_info info;wimlib_get_wim_info(w,&info);info.is_marked_readonly=1;wimlib_set_wim_info(w,&info,WIMLIB_CHANGE_READONLY_FLAG);add(w,"readonly-add");del(w,1);del(w,-1);del(w,-1);
 wimlib_free(w);return 0;
}
