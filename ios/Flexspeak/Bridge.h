#pragma once

typedef struct Session FlexspeakSession;
typedef void (*FlexspeakCallback)(const char *json, void *context);

FlexspeakSession *flexspeak_connect(const char *address, const char *nickname, const char *password,
                                    FlexspeakCallback callback, void *context);
void flexspeak_command(FlexspeakSession *session, const char *json);
void flexspeak_disconnect(FlexspeakSession *session);
_Bool flexspeak_frame(FlexspeakSession *session, unsigned long slot, unsigned char *buffer,
                      unsigned long capacity, unsigned int *width, unsigned int *height);
void flexspeak_push_screen(const unsigned char *y, unsigned long y_length, unsigned int y_stride,
                           const unsigned char *uv, unsigned long uv_length, unsigned int uv_stride,
                           unsigned int width, unsigned int height, unsigned int rotation);
