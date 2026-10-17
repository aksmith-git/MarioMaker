// Decompile functions to text files for behavioural analysis.
//
// Usage (headless, -process on an analysed program):
//   -postScript Decompile.java <outdir> <depth> <addr|name> [<addr|name> ...]
// depth 0 = only the named functions; depth N also decompiles callees N levels down
// (skipping SDK imports/thunks). Writes <outdir>/<entry>_<name>.c plus <outdir>/index.tsv
// (entry, name, size, callers, callees).
//
// Output is decompiler pseudocode of the user's own game binary and belongs in research/.
// @category SMM

import java.io.File;
import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.LinkedHashSet;
import java.util.Set;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.util.task.TaskMonitor;

public class Decompile extends GhidraScript {

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 3) {
            printerr("usage: Decompile.java <outdir> <depth> <addr|name>...");
            return;
        }
        File outDir = new File(args[0]);
        outDir.mkdirs();
        int maxDepth = Integer.parseInt(args[1]);

        DecompInterface decomp = new DecompInterface();
        decomp.openProgram(currentProgram);

        Deque<Object[]> queue = new ArrayDeque<>();
        for (int i = 2; i < args.length; i++) {
            if (args[i].startsWith("range:")) {  // range:<start>-<end> decompiles every function inside
                String[] r = args[i].substring(6).split("-");
                Address lo = toAddr(r[0].replaceFirst("^0x", "")), hi = toAddr(r[1].replaceFirst("^0x", ""));
                for (Function f : currentProgram.getFunctionManager().getFunctions(lo, true)) {
                    if (f.getEntryPoint().compareTo(hi) >= 0) {
                        break;
                    }
                    queue.add(new Object[] { f, 0 });
                }
                continue;
            }
            Function f = resolve(args[i]);
            if (f == null) {
                printerr("not found: " + args[i]);
                continue;
            }
            queue.add(new Object[] { f, 0 });
        }

        Set<Address> done = new LinkedHashSet<>();
        try (PrintWriter index = new PrintWriter(new FileWriter(new File(outDir, "index.tsv"), true))) {
            while (!queue.isEmpty() && !monitor.isCancelled()) {
                Object[] item = queue.poll();
                Function f = (Function) item[0];
                int depth = (Integer) item[1];
                if (f.isThunk() || f.isExternal() || !done.add(f.getEntryPoint())) {
                    continue;
                }
                DecompileResults res = decomp.decompileFunction(f, 120, TaskMonitor.DUMMY);
                String code = res.decompileCompleted() ? res.getDecompiledFunction().getC() : "// decompile failed: " + res.getErrorMessage();
                String file = f.getEntryPoint().toString() + "_" + f.getName().replaceAll("[^A-Za-z0-9_.]", "_") + ".c";
                try (PrintWriter w = new PrintWriter(new FileWriter(new File(outDir, file)))) {
                    w.println("// " + f.getName() + " @ " + f.getEntryPoint() + "  size=" + f.getBody().getNumAddresses());
                    for (Function c : f.getCallingFunctions(TaskMonitor.DUMMY)) {
                        w.println("// caller: " + c.getEntryPoint() + " " + c.getName());
                    }
                    w.println(code);
                }
                Set<Function> callees = f.getCalledFunctions(TaskMonitor.DUMMY);
                StringBuilder cs = new StringBuilder();
                for (Function c : callees) {
                    cs.append(c.getEntryPoint()).append(' ');
                    if (depth < maxDepth) {
                        queue.add(new Object[] { c, depth + 1 });
                    }
                }
                index.printf("%s\t%s\t%d\t%d\t%s%n", f.getEntryPoint(), f.getName(), f.getBody().getNumAddresses(),
                        f.getCallingFunctions(TaskMonitor.DUMMY).size(), cs.toString().trim());
            }
        }
        decomp.dispose();
        println("decompiled " + done.size() + " functions to " + outDir);
    }

    private Function resolve(String s) {
        try {
            Address a = toAddr(s.startsWith("0x") ? s.substring(2) : s);
            Function f = getFunctionContaining(a);
            if (f != null) {
                return f;
            }
        } catch (Exception ignored) {
        }
        return getGlobalFunctions(s).isEmpty() ? null : getGlobalFunctions(s).get(0);
    }
}
