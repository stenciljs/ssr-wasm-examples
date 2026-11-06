// Downloads a file and checks its SHA-256: `java Fetch.java <url> <sha256> <out>`.
// The JDK image has no curl or wget; this uses java.net.http (and the JVM truststore, so
// with-proxy-ca.sh works for it too).
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;

class Fetch {
    public static void main(String[] args) throws Exception {
        var client = HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NORMAL).build();
        var res = client.send(HttpRequest.newBuilder(URI.create(args[0])).build(), HttpResponse.BodyHandlers.ofByteArray());
        if (res.statusCode() != 200) throw new IllegalStateException("HTTP " + res.statusCode() + " for " + args[0]);
        String sha = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(res.body()));
        if (!sha.equalsIgnoreCase(args[1])) throw new IllegalStateException("sha256 mismatch for " + args[0] + ": " + sha);
        Files.write(Path.of(args[2]), res.body());
    }
}
