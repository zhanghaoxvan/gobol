/*
 * channel.c — Thread-safe message queue.
 */
#include "platform.h"
#include "channel.h"

typedef struct _gobol_chan_node {
    long long data;
    struct _gobol_chan_node *next;
} _gobol_chan_node;

typedef struct {
    gobol_mutex_t lock;
    gobol_cond_t  cond;
    _gobol_chan_node *head;
    _gobol_chan_node *tail;
    int closed;
} _gobol_chan;

long long gobol_chan_create(void) {
    _gobol_chan *ch = (_gobol_chan *)malloc(sizeof(_gobol_chan));
    if (!ch) return 0;
    gobol_mutex_init(&ch->lock);
    gobol_cond_init(&ch->cond);
    ch->head = NULL;
    ch->tail = NULL;
    ch->closed = 0;
    return (long long)ch;
}

long long gobol_chan_send(long long chan_id, long long data) {
    if (!chan_id) return -1;
    _gobol_chan *ch = (_gobol_chan *)chan_id;
    gobol_mutex_lock(&ch->lock);
    if (ch->closed) { gobol_mutex_unlock(&ch->lock); return -1; }
    _gobol_chan_node *node = (_gobol_chan_node *)malloc(sizeof(_gobol_chan_node));
    if (!node) { gobol_mutex_unlock(&ch->lock); return -1; }
    node->data = data;
    node->next = NULL;
    if (ch->tail) ch->tail->next = node;
    else          ch->head = node;
    ch->tail = node;
    gobol_cond_signal(&ch->cond);
    gobol_mutex_unlock(&ch->lock);
    return 0;
}

long long gobol_chan_recv(long long chan_id) {
    if (!chan_id) return 0;
    _gobol_chan *ch = (_gobol_chan *)chan_id;
    gobol_mutex_lock(&ch->lock);
    while (!ch->head && !ch->closed)
        gobol_cond_wait(&ch->cond, &ch->lock);
    if (!ch->head) { gobol_mutex_unlock(&ch->lock); return 0; }
    _gobol_chan_node *node = ch->head;
    ch->head = node->next;
    if (!ch->head) ch->tail = NULL;
    long long data = node->data;
    free(node);
    gobol_mutex_unlock(&ch->lock);
    return data;
}

void gobol_chan_destroy(long long chan_id) {
    if (!chan_id) return;
    _gobol_chan *ch = (_gobol_chan *)chan_id;
    gobol_mutex_lock(&ch->lock);
    ch->closed = 1;
    gobol_cond_broadcast(&ch->cond);
    _gobol_chan_node *n = ch->head;
    while (n) {
        _gobol_chan_node *next = n->next;
        free(n);
        n = next;
    }
    gobol_mutex_unlock(&ch->lock);
    gobol_mutex_destroy(&ch->lock);
    gobol_cond_destroy(&ch->cond);
    free(ch);
}
