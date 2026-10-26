#import <Cocoa/Cocoa.h>
#import <WebKit/WebKit.h>
@interface WKWebView (NativeInput) <NSTextInputClient>
- (void)_setWindowOcclusionDetectionEnabled:(BOOL)enabled;
@end
// WebKit에는 활성 화면으로 보이지만 NSWindow를 실제 디스플레이에 올리지 않는다.
@interface OffscreenWindow : NSWindow
@end
@implementation OffscreenWindow
- (BOOL)isVisible { return YES; }
- (BOOL)isKeyWindow { return YES; }
- (NSWindowOcclusionState)occlusionState { return NSWindowOcclusionStateVisible; }
@end
static void reply(NSDictionary *value) {
 NSData *data = [NSJSONSerialization dataWithJSONObject:value options:0 error:NULL];
 fwrite(data.bytes,1,data.length,stdout); fputc('\n',stdout); fflush(stdout);
}
@interface Driver : NSObject <WKNavigationDelegate, WKScriptMessageHandler>
@property WKWebView *view;
@property NSWindow *window;
@property NSNumber *loading;
@end
@implementation Driver
- (void)userContentController:(WKUserContentController *)c didReceiveScriptMessage:(WKScriptMessage *)m { reply(m.body); }
- (void)webView:(WKWebView *)view didFinishNavigation:(WKNavigation *)n { if(self.loading) {reply(@{@"id":self.loading,@"result":@YES});self.loading=nil;} }
- (void)webView:(WKWebView *)view didFailProvisionalNavigation:(WKNavigation *)n withError:(NSError *)e {reply(@{@"id":self.loading?:@0,@"error":e.localizedDescription}); self.loading=nil;}
- (void)command:(NSDictionary *)c {
 NSNumber *ident=c[@"id"];
 NSString *op=c[@"op"];
 if([op isEqual:@"load"]) { self.loading=ident; [self.view loadRequest:[NSURLRequest requestWithURL:[NSURL URLWithString:c[@"url"]]]]; }
 else if([op isEqual:@"eval"]) {
  NSString *script=[NSString stringWithFormat:@"Promise.resolve().then(async()=>{%@}).then(result=>window.webkit.messageHandlers.reply.postMessage({id:%@,result:result??null}),e=>window.webkit.messageHandlers.reply.postMessage({id:%@,error:String(e)}));void 0",c[@"js"],ident,ident];
  [self.view evaluateJavaScript:script completionHandler:^(id value,NSError *error){if(error)reply(@{@"id":ident,@"error":error.localizedDescription});}];
 } else if([op isEqual:@"mark"]) {
  NSString *text=c[@"text"]; [self.window makeFirstResponder:self.view];
  [self.view setMarkedText:text selectedRange:NSMakeRange(text.length,0) replacementRange:NSMakeRange(NSNotFound,0)];
  reply(@{@"id":ident,@"result":@YES});
 } else if([op isEqual:@"commit"]) {
  [self.view insertText:c[@"text"] replacementRange:NSMakeRange(NSNotFound,0)]; reply(@{@"id":ident,@"result":@YES});
 } else if([op isEqual:@"shot"]) {
  [self.view takeSnapshotWithConfiguration:nil completionHandler:^(NSImage *image,NSError *error){
   if(error){reply(@{@"id":ident,@"error":error.localizedDescription});return;}
   NSBitmapImageRep *bitmap=[NSBitmapImageRep imageRepWithData:image.TIFFRepresentation];
   NSData *png=[bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
   BOOL ok=[png writeToFile:c[@"path"] atomically:YES]; reply(@{@"id":ident,@"result":@(ok)});
  }];
 } else if([op isEqual:@"quit"]) { reply(@{@"id":ident,@"result":@YES});[NSApp terminate:nil]; }
}
@end
int main(void) { @autoreleasepool {
 [NSApplication sharedApplication]; [NSApp setActivationPolicy:NSApplicationActivationPolicyProhibited];
 Driver *driver=[Driver new];
 WKWebViewConfiguration *config=[WKWebViewConfiguration new];
 config.websiteDataStore=WKWebsiteDataStore.nonPersistentDataStore;
 [config.userContentController addScriptMessageHandler:driver name:@"reply"];
 driver.view=[[WKWebView alloc] initWithFrame:NSMakeRect(0,0,1280,800) configuration:config];
 driver.view.navigationDelegate=driver;
 [driver.view _setWindowOcclusionDetectionEnabled:NO];
 driver.window=[[OffscreenWindow alloc] initWithContentRect:NSMakeRect(0,0,1280,800) styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:NO];
 driver.window.contentView=driver.view;
 [driver.window makeFirstResponder:driver.view];
 NSFileHandle *input=[NSFileHandle fileHandleWithStandardInput];
 NSMutableData *pending=[NSMutableData new];
 input.readabilityHandler=^(NSFileHandle *handle){
  NSData *part=handle.availableData;
  if(!part.length){dispatch_async(dispatch_get_main_queue(),^{[NSApp terminate:nil];});return;}
  [pending appendData:part];
  while(YES){const char *bytes=pending.bytes;const char *end=memchr(bytes,'\n',pending.length);if(!end)break;
   NSUInteger length=end-bytes;NSData *line=[pending subdataWithRange:NSMakeRange(0,length)];
   [pending replaceBytesInRange:NSMakeRange(0,length+1) withBytes:NULL length:0];
   NSDictionary *command=[NSJSONSerialization JSONObjectWithData:line options:0 error:NULL];
   dispatch_async(dispatch_get_main_queue(),^{[driver command:command];});
  }
 };
 reply(@{@"ready":@YES}); [NSApp run];
} return 0; }
