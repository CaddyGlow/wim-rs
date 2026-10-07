/* Extend the original-header capture client with real writer policy choices. */
#include <wimlib.h>
#include <stdlib.h>
static int capture_write(WIMStruct *wim,const wimlib_tchar *path,int image,int flags,unsigned threads)
{
 const char *value=getenv("WIM_CAPTURE_WRITE_FLAGS");
 return wimlib_write(wim,path,image,flags|(value?(int)strtol(value,NULL,0):0),threads);
}
static int capture_create(int codec,WIMStruct **output)
{
 const char *value=getenv("WIM_CAPTURE_CODEC");
 return wimlib_create_new_wim(value?(int)strtol(value,NULL,0):codec,output);
}
#define wimlib_write capture_write
#define wimlib_create_new_wim capture_create
#include "probe-capture-api.c"
