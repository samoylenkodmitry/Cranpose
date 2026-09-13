#import "ReferenceGesture.h"

@interface XCPointerEventPath : NSObject
- (instancetype)initForTouchAtPoint:(CGPoint)point offset:(double)offset;
- (void)moveToPoint:(CGPoint)point atOffset:(double)offset;
- (void)liftUpAtOffset:(double)offset;
@end

@interface XCSynthesizedEventRecord : NSObject <NSSecureCoding>
- (instancetype)initWithName:(NSString *)name interfaceOrientation:(NSInteger)orientation;
- (void)addPointerEventPath:(XCPointerEventPath *)path;
- (BOOL)synthesizeWithError:(NSError **)error;
@end

@implementation ReferenceGesture
+ (NSData *)synthesizePoints:(NSArray<NSValue *> *)points
                    offsets:(NSArray<NSNumber *> *)offsets
                       name:(NSString *)name
                      error:(NSError **)error {
    BOOL valid = points.count >= 4 && points.count == offsets.count;
    for (NSUInteger index = 0; valid && index < points.count; index++) {
        CGPoint point = points[index].CGPointValue;
        double time = offsets[index].doubleValue;
        valid = isfinite(point.x) && isfinite(point.y) && isfinite(time)
            && (index == 0 ? time == 0 : time > offsets[index - 1].doubleValue);
    }
    valid = valid && CGPointEqualToPoint(points.lastObject.CGPointValue,
                                         points[points.count - 2].CGPointValue);
    Class pathClass = NSClassFromString(@"XCPointerEventPath");
    Class recordClass = NSClassFromString(@"XCSynthesizedEventRecord");
    if (!valid || ![pathClass instancesRespondToSelector:@selector(initForTouchAtPoint:offset:)]
        || ![recordClass instancesRespondToSelector:@selector(synthesizeWithError:)]) {
        if (error) {
            *error = [NSError errorWithDomain:@"ReferenceGesture" code:1
                                    userInfo:@{NSLocalizedDescriptionKey: @"Invalid continuous touch path or unavailable XCTest event synthesis"}];
        }
        return nil;
    }
    XCPointerEventPath *path = [[pathClass alloc] initForTouchAtPoint:points[0].CGPointValue offset:0];
    for (NSUInteger index = 1; index + 1 < points.count; index++) {
        [path moveToPoint:points[index].CGPointValue atOffset:offsets[index].doubleValue];
    }
    [path liftUpAtOffset:offsets.lastObject.doubleValue];
    XCSynthesizedEventRecord *record = [[recordClass alloc] initWithName:name interfaceOrientation:UIInterfaceOrientationPortrait];
    [record addPointerEventPath:path];
    NSData *archive = [NSKeyedArchiver archivedDataWithRootObject:record requiringSecureCoding:YES error:error];
    if (!archive || ![record synthesizeWithError:error]) {
        return nil;
    }
    return archive;
}
@end
