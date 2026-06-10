# Keep kotlinx.serialization runtime metadata for our @Serializable types.
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.AnnotationsKt
-keep,includedescriptorclasses class im.zyx.phonebridge.**$$serializer { *; }
-keepclassmembers class im.zyx.phonebridge.** {
    *** Companion;
}
-keepclasseswithmembers class im.zyx.phonebridge.** {
    kotlinx.serialization.KSerializer serializer(...);
}

# Ktor transitively pulls in slf4j via its logging facade. We never
# configure a binding, so silence the missing-class warning.
-dontwarn org.slf4j.**
