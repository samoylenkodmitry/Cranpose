#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>

NS_ASSUME_NONNULL_BEGIN
@interface ReferenceGesture : NSObject
+ (nullable NSData *)synthesizePoints:(NSArray<NSValue *> *)points
                             offsets:(NSArray<NSNumber *> *)offsets
                                name:(NSString *)name
                               error:(NSError **)error NS_SWIFT_NAME(synthesize(points:offsets:name:));
@end
NS_ASSUME_NONNULL_END
