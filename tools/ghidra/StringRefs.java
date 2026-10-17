// List the functions that reference strings matching a regex.
//
// Usage: -postScript StringRefs.java <outfile.tsv> <regex>
// Writes: string address, string, referencing address, referencing function entry, function name.
// Scans defined strings plus raw ASCII runs in read-only data, because the analyser does not
// define every literal.
// @category SMM

import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.regex.Pattern;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.Reference;

public class StringRefs extends GhidraScript {

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        Pattern pat = Pattern.compile(args[1]);
        int hits = 0;
        try (PrintWriter out = new PrintWriter(new FileWriter(args[0]))) {
            DataIterator it = currentProgram.getListing().getDefinedData(true);
            while (it.hasNext() && !monitor.isCancelled()) {
                Data d = it.next();
                if (!d.hasStringValue()) {
                    continue;
                }
                String s = String.valueOf(d.getValue());
                if (pat.matcher(s).find()) {
                    hits += emit(out, d.getAddress(), s);
                }
            }
            // Undefined literals: scan read-only blocks for NUL-terminated ASCII runs.
            for (MemoryBlock b : currentProgram.getMemory().getBlocks()) {
                if (b.isExecute() || b.isWrite() || !b.isInitialized()) {
                    continue;
                }
                byte[] bytes = new byte[(int) b.getSize()];
                b.getBytes(b.getStart(), bytes);
                int start = -1;
                for (int i = 0; i < bytes.length; i++) {
                    int c = bytes[i] & 0xff;
                    if (c >= 0x20 && c < 0x7f) {
                        if (start < 0) start = i;
                    } else {
                        if (c == 0 && start >= 0 && i - start >= 4) {
                            Address a = b.getStart().add(start);
                            if (getDataAt(a) == null || !getDataAt(a).hasStringValue()) {
                                String s = new String(bytes, start, i - start, "US-ASCII");
                                if (pat.matcher(s).find()) {
                                    hits += emit(out, a, s);
                                }
                            }
                        }
                        start = -1;
                    }
                }
            }
        }
        println(hits + " references written to " + args[0]);
    }

    private int emit(PrintWriter out, Address a, String s) {
        int n = 0;
        for (Reference r : getReferencesTo(a)) {
            Function f = getFunctionContaining(r.getFromAddress());
            out.printf("%s\t%s\t%s\t%s\t%s%n", a, s.replace('\t', ' '), r.getFromAddress(),
                    f == null ? "-" : f.getEntryPoint(), f == null ? "-" : f.getName());
            n++;
        }
        if (n == 0) {
            out.printf("%s\t%s\t-\t-\t-%n", a, s.replace('\t', ' '));
        }
        return Math.max(n, 1);
    }
}
