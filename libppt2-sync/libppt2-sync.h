#include <stdbool.h>

typedef struct Ppt2Sync Ppt2Sync;

/* Initializes the synchronizer.
 * 
 * If an error occurs, it is printed to standard error and this function returns NULL.
 * 
 * `ppt2-sync.exe` must exist in the working directory of your program.
 */
Ppt2Sync *ppt2sync_new();

/* Waits until PPT2 reaches the next frame.
 * 
 * PPT2 will be blocked until the next call to this function, so do not do blocking operations
 * or expensive computation between calls to this function.
 * 
 * This function returns false if it can't communicate with the synchronizer. This usually happens
 * when PPT2 closes. Once this function returns false, you should destroy the synchronizer.
 */
bool ppt2sync_wait_for_frame(Ppt2Sync *ppt2sync);

/* Cleanup the synchronizer. */
void ppt2sync_destroy(Ppt2Sync *ppt2sync);
