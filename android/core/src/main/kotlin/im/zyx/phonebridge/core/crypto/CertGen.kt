package im.zyx.phonebridge.core.crypto

import java.io.StringWriter
import java.math.BigInteger
import java.security.KeyPair
import java.security.MessageDigest
import java.security.Security
import java.security.cert.X509Certificate
import java.util.Date
import org.bouncycastle.asn1.x500.X500Name
import org.bouncycastle.asn1.x509.BasicConstraints
import org.bouncycastle.asn1.x509.Extension
import org.bouncycastle.cert.jcajce.JcaX509CertificateConverter
import org.bouncycastle.cert.jcajce.JcaX509v3CertificateBuilder
import org.bouncycastle.openssl.jcajce.JcaPEMWriter
import org.bouncycastle.operator.jcajce.JcaContentSignerBuilder

/**
 * Self-signed X.509 certificate generation for PhoneBridge device
 * identity. Produces a wire-compatible PEM and SHA-256 fingerprint
 * matching the Rust `cert::generate_self_signed` and
 * `fingerprint::cert_fingerprint_der`.
 *
 * Uses BouncyCastle because Android's stock JCE does not expose X.509
 * certificate building (only parsing). BC is registered as a
 * provider on first use.
 */
object CertGen {
    init {
        // Add the BC provider once. Idempotent: Security.addProvider
        // is a no-op if the same name is already registered.
        if (Security.getProvider("BC") == null) {
            Security.addProvider(org.bouncycastle.jce.provider.BouncyCastleProvider())
        }
    }

    data class SelfSignedCert(
        /** The X.509 certificate (PEM-encoded, contains "BEGIN CERTIFICATE"). */
        val pem: String,
        /** SHA-256 of the DER-encoded cert, formatted as 32 colon-separated UPPERCASE hex pairs. */
        val fingerprint: String,
        /** The DER-encoded cert bytes, used to compute the fingerprint. */
        val der: ByteArray
    )

    /**
     * Generate a self-signed P-256 certificate valid for [validityDays]
     * (default 10 years, matching the Rust side's 3650 days).
     */
    fun generateSelfSigned(
        commonName: String,
        keyPair: KeyPair,
        validityDays: Int = 3650
    ): SelfSignedCert {
        val now = System.currentTimeMillis()
        val notBefore = Date(now)
        val notAfter = Date(now + validityDays.toLong() * 24L * 3600L * 1000L)
        val name = X500Name("CN=$commonName")
        // Serial must be positive; use a deterministic high-entropy
        // value derived from the public key to keep tests stable.
        val pub = (keyPair.public as java.security.interfaces.ECPublicKey)
        val pubHash = MessageDigest.getInstance("SHA-256").digest(Ecdh.uncompressedBytes(pub))
        val serial = BigInteger(1, pubHash).abs().let { if (it == BigInteger.ZERO) BigInteger.ONE else it }

        val builder = JcaX509v3CertificateBuilder(
            name,
            serial,
            notBefore,
            notAfter,
            name,
            keyPair.public
        )
        // ca: false — this is an end-entity cert, not a CA.
        builder.addExtension(Extension.basicConstraints, true, BasicConstraints(false))
        val signer = JcaContentSignerBuilder("SHA256withECDSA").build(keyPair.private)
        val holder = builder.build(signer)
        val cert: X509Certificate = JcaX509CertificateConverter().getCertificate(holder)

        val pem = pemEncode(cert)
        val der = cert.encoded
        val fp = fingerprint(der)
        return SelfSignedCert(pem = pem, fingerprint = fp, der = der)
    }

    private fun pemEncode(cert: X509Certificate): String {
        val sw = StringWriter()
        JcaPEMWriter(sw).use { it.writeObject(cert) }
        return sw.toString()
    }

    /** 32 bytes -> 32 colon-separated UPPERCASE hex pairs. */
    fun fingerprint(der: ByteArray): String {
        val d = MessageDigest.getInstance("SHA-256").digest(der)
        val sb = StringBuilder(d.size * 3)
        for ((i, b) in d.withIndex()) {
            if (i > 0) sb.append(':')
            sb.append("%02X".format(b))
        }
        return sb.toString()
    }
}
