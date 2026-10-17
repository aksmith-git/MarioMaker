// Dump a table of 8-byte pointers (e.g. a vtable) after relocation, with target function info.
//
// Usage: -postScript Pointers.java <outfile.tsv> <addr> <count>
// Writes: slot, slot address, target, target function entry, name, size.
// @category SMM

import java.io.FileWriter;
import java.io.PrintWriter;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

public class Pointers extends GhidraScript {

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        Address base = toAddr(args[1].replaceFirst("^0x", ""));
        int count = Integer.parseInt(args[2]);
        try (PrintWriter out = new PrintWriter(new FileWriter(args[0]))) {
            for (int i = 0; i < count; i++) {
                Address slot = base.add(i * 8L);
                long v = getLong(slot);
                Address target = toAddr(v);
                Function f = getFunctionAt(target);
                if (f == null) {
                    f = getFunctionContaining(target);
                }
                out.printf("%d\t%s\t%s\t%s\t%s\t%d%n", i, slot, target, f == null ? "-" : f.getEntryPoint(),
                        f == null ? "-" : f.getName(), f == null ? 0 : f.getBody().getNumAddresses());
            }
        }
        println("pointers written to " + args[0]);
    }
}
