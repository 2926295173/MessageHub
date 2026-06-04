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
