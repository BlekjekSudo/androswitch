import java.util.*;
public final class CollectionsFactoryOracle {
  static void row(String key, Object value) { System.out.println(key + "=" + value); }
  static boolean rejects(Runnable body) { try { body.run(); return false; } catch (UnsupportedOperationException expected) { return true; } }
  public static void main(String[] args) {
    row("emptyList.size", Collections.emptyList().size());
    row("emptySet.size", Collections.emptySet().size());
    row("emptyMap.size", Collections.emptyMap().size());
    row("singletonList.null", Collections.singletonList(null).get(0) == null);
    row("singleton.null", Collections.singleton(null).contains(null));
    row("singletonList.rejectsAdd", rejects(() -> Collections.singletonList(null).add("x")));
    row("emptyList.rejectsAdd", rejects(() -> Collections.emptyList().add(null)));
    row("emptySet.rejectsAdd", rejects(() -> Collections.emptySet().add(null)));
    row("emptyMap.rejectsPut", rejects(() -> Collections.emptyMap().put(null,null)));
    row("EMPTY_LIST.identity", Collections.EMPTY_LIST == Collections.EMPTY_LIST);
    List<String> source = new ArrayList<>(); source.add("kept");
    List<String> view = Collections.unmodifiableList(source); source.clear();
    row("unmodifiableList.sizeAfterSourceClear", view.size());
    row("emptySortedSet.isSortedSet", Collections.emptySortedSet() instanceof SortedSet);
    row("emptySortedMap.isSortedMap", Collections.emptySortedMap() instanceof SortedMap);
    try { Collections.unmodifiableMap(null); row("unmodifiableMap.null", "accepted"); }
    catch (NullPointerException expected) { row("unmodifiableMap.null", "NullPointerException"); }
  }
}
