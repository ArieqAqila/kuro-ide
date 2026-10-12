#ifdef INDEX_INFORMATION
dmoidx   dmokode
#endif /* INDEX_INFORMATION */

#define dmodat 	-1600
#define dmoidx            -1601

typedef struct dmo { /* Demo table */
LONG           dmoprm;                  /*0,Key*/
TEXT           dmokode[11];             /*4,Code*/
TEXT           dmoname[51];             /*15,Name*/
double         dmoamount;               /*66,Amount*/
TEXT           dmostatus[2];            /*74,Status*/
} DMO;
