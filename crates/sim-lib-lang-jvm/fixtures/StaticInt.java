public final class StaticInt {
    private StaticInt() {}

    public static int wholePipeline(int left, int right) {
        int sum = left + right;
        return sum * 2;
    }

    public static long nonIntParameter(long value) { return value; }
    public static void nonIntReturn(int value) {}
    public int instance(int value) { return value; }
}
